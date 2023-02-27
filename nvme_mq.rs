use core::ffi::c_void;
use core::{marker::PhantomPinned, mem::zeroed, pin::Pin, ptr};
use kernel::bindings;
use kernel::driver;
use kernel::irq;
use kernel::prelude::*;
use kernel::sync::Arc;
use kernel::Result;
use kernel::{c_str, str::CStr};

use crate::nvme_queue::*;
use crate::{nvme_defs::*, NvmeTraitsImpl};

pub const MAX_SG: usize = 16;

pub struct BlkDevOps;

impl BlkDevOps {
    pub const OPS: bindings::block_device_operations = bindings::block_device_operations {
        submit_bio: None,
        open: None,
        release: None,
        rw_page: None,
        ioctl: None,
        compat_ioctl: None,
        check_events: None,
        unlock_native_capacity: None,
        getgeo: None,
        set_read_only: None,
        swap_slot_free_notify: None,
        report_zones: None,
        devnode: None,
        alternative_gpt_sector: None,
        get_unique_id: None,
        owner: core::ptr::null_mut(),
        pr_ops: core::ptr::null_mut(),
        free_disk: None,
        poll_bio: None,
    };
}

pub trait BlkRqOps {
    unsafe extern "C" fn init_request(
        _set: *mut bindings::blk_mq_tag_set,
        rq: *mut bindings::request,
        _arg2: core::ffi::c_uint,
        _arg3: core::ffi::c_uint,
    ) -> core::ffi::c_int;

    unsafe extern "C" fn init_hctx(
        _arg1: *mut bindings::blk_mq_hw_ctx,
        _arg2: *mut core::ffi::c_void,
        _arg3: core::ffi::c_uint,
    ) -> core::ffi::c_int;

    unsafe extern "C" fn queue_rq(
        arg1: *mut bindings::blk_mq_hw_ctx,
        bd: *const bindings::blk_mq_queue_data,
    ) -> bindings::blk_status_t;

    unsafe extern "C" fn complete(req: *mut bindings::request);
}

pub struct Adapter<T: BlkRqOps>(T);

impl<T: BlkRqOps> Adapter<T> {
    unsafe extern "C" fn init_request(
        set: *mut bindings::blk_mq_tag_set,
        rq: *mut bindings::request,
        arg2: core::ffi::c_uint,
        arg3: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        unsafe { T::init_request(set, rq, arg2, arg3) }
    }
    unsafe extern "C" fn init_hctx(
        arg1: *mut bindings::blk_mq_hw_ctx,
        arg2: *mut core::ffi::c_void,
        arg3: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        unsafe { T::init_hctx(arg1, arg2, arg3) }
    }

    unsafe extern "C" fn queue_rq(
        arg1: *mut bindings::blk_mq_hw_ctx,
        bd: *const bindings::blk_mq_queue_data,
    ) -> bindings::blk_status_t {
        unsafe { T::queue_rq(arg1, bd) }
    }

    unsafe extern "C" fn complete(req: *mut bindings::request) {
        unsafe { T::complete(req) }
    }

    const OPS: bindings::blk_mq_ops = bindings::blk_mq_ops {
        queue_rq: Some(Self::queue_rq),
        commit_rqs: None,
        queue_rqs: None,
        put_budget: None,
        get_budget: None,
        set_rq_budget_token: None,
        get_rq_budget_token: None,
        timeout: None,
        poll: None,
        complete: Some(Self::complete),
        init_hctx: Some(Self::init_hctx),
        exit_hctx: None,
        init_request: Some(Self::init_request),
        exit_request: None,
        cleanup_rq: None,
        busy: None,
        map_queues: None,
        show_rq: None,
    };
}

#[derive(Clone, Copy)]
pub struct Tag {
    pub set: bindings::blk_mq_tag_set,
    _pin: PhantomPinned,
}

impl Tag {
    pub fn new<T: BlkRqOps>() -> Self {
        let mut set: bindings::blk_mq_tag_set = unsafe { zeroed() };
        set.ops = &Adapter::<T>::OPS;

        Tag {
            set,
            _pin: PhantomPinned,
        }
    }
}

#[repr(C)]
pub struct Pdu {
    pub cmnd: NvmeCommand,
    pub status: u16,
    pub result: u32,
    pub nents: i32,

    pub sg: [bindings::scatterlist; MAX_SG],
}

impl Default for Pdu {
    fn default() -> Self {
        Self {
            cmnd: NvmeCommand {
                common: NvmeCommon {
                    ..NvmeCommon::default()
                },
            },
            status: 0,
            result: 0,
            nents: 0,
            sg: [bindings::scatterlist::default(); MAX_SG],
        }
    }
}

pub fn rq_to_pdu(rq: *mut bindings::request) -> *mut Pdu {
    let ptr = unsafe { bindings::blk_mq_rq_to_pdu(rq) };
    ptr as *mut Pdu
}

pub struct RequestQueue {
    pub ptr: *mut bindings::request_queue,
}

impl RequestQueue {
    pub fn new(tag: &mut Tag) -> Self {
        unsafe {
            tag.set.nr_hw_queues = 1;
            tag.set.queue_depth = 1024;
            tag.set.timeout = 30 * bindings::HZ;
            tag.set.numa_node = -1;
            tag.set.cmd_size = core::mem::size_of::<Pdu>() as u32;
            tag.set.flags = bindings::BLK_MQ_F_NO_SCHED;
            let _r = bindings::blk_mq_alloc_tag_set(&mut tag.set);
        }

        let ptr = unsafe { bindings::blk_mq_init_queue(&mut tag.set) };
        if unsafe { bindings::IS_ERR(ptr as *const c_void) } {
            pr_info!("init queue error");
        }
        RequestQueue { ptr }
    }
}

pub struct BlkAdminOps;

impl BlkRqOps for BlkAdminOps {
    unsafe extern "C" fn init_request(
        _set: *mut bindings::blk_mq_tag_set,
        _rq: *mut bindings::request,
        _arg2: core::ffi::c_uint,
        _arg3: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        0
    }

    unsafe extern "C" fn init_hctx(
        _arg1: *mut bindings::blk_mq_hw_ctx,
        _arg2: *mut core::ffi::c_void,
        _arg3: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        0
    }

    unsafe extern "C" fn queue_rq(
        hctx: *mut bindings::blk_mq_hw_ctx,
        bd: *const bindings::blk_mq_queue_data,
    ) -> bindings::blk_status_t {
        pr_info!("----------adminqueue queue rq---------------");
        unsafe {
            let req = (*bd).rq;
            bindings::blk_mq_start_request(req);
            let pdu = rq_to_pdu(req);

            let queue: *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>> =
                (*(*req).q).queuedata as *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>>;
            let cmd = &((*pdu).cmnd);
            (*queue).submit_command(&cmd, true);

            let mut a = Box::from_raw(
                (*(*req).q).queuedata as *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>>,
            );
            drop(queue);
            (*(*req).q).queuedata = Box::into_raw(a) as _;
        }
        bindings::BLK_STS_OK as _
    }

    unsafe extern "C" fn complete(req: *mut bindings::request) {
        pr_info!("process complete");

        unsafe {
            bindings::blk_mq_end_request(req, bindings::BLK_STS_OK as u8);
        }
    }
}

pub const CTRL_PAGE_SIZE: i32 = 4096;
pub const NVME_CTRL_PAGE_SIZE: usize = kernel::PAGE_SIZE;
static mut DEV_PTR_FIRST: usize = 0;

pub struct BlkIoOps;

impl BlkRqOps for BlkIoOps {
    unsafe extern "C" fn init_request(
        set: *mut bindings::blk_mq_tag_set,
        rq: *mut bindings::request,
        _hctx_idx: core::ffi::c_uint,
        _numa_node: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        0
    }

    unsafe extern "C" fn init_hctx(
        _arg1: *mut bindings::blk_mq_hw_ctx,
        _arg2: *mut core::ffi::c_void,
        _arg3: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        0
    }

    unsafe extern "C" fn queue_rq(
        hctx: *mut bindings::blk_mq_hw_ctx,
        bd: *const bindings::blk_mq_queue_data,
    ) -> bindings::blk_status_t {
        pr_info!("----------ioqueue queue rq---------------");
        unsafe {
            let req = (*bd).rq;
            bindings::blk_mq_start_request(req);
            let pdu = rq_to_pdu(req);
            (*pdu).nents = 0;
            let mut sg_idx = 0;

            let queue: *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>> =
                (*(*req).q).queuedata as *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>>;

            let mut dev: *mut bindings::device = (*queue).device;

            let mut length: i32 = bindings::blk_rq_payload_bytes(req) as i32;

            let bio = (*req).bio as *mut bindings::bio;
            let offset = unsafe { (*bio).bi_iter.bi_sector };
            let dir = if ((bindings::REQ_OP_BITS & (*req).cmd_flags) & 1) == 1 {
                bindings::dma_data_direction_DMA_TO_DEVICE
            } else {
                bindings::dma_data_direction_DMA_FROM_DEVICE
            };
            let opcode = if dir == bindings::dma_data_direction_DMA_TO_DEVICE {
                NvmeOpcode::read
            } else {
                NvmeOpcode::write
            };

            let cmd_id = (*req).tag as u16;

            pr_info!("cmd_id {:?}", cmd_id);

            let len = length;
            let mut cmd = NvmeCommand {
                rw: NvmeRw {
                    opcode: opcode as u8,
                    command_id: cmd_id,
                    nsid: 1.into(),
                    slba: bindings::blk_rq_pos(req).into(),
                    length: (((bindings::blk_rq_bytes(req) >> 9) - 1) as u16).into(),
                    ..NvmeRw::default()
                },
            };
            let n = bindings::blk_rq_nr_phys_segments(req);

            if n >= 1 {
                // let mut last_sg = core::ptr::null_mut();
                bindings::sg_init_table(&mut (*pdu).sg[sg_idx], n as u32);

                let nents = bindings::blk_rq_map_sg((*req).q, req, &mut (*pdu).sg[sg_idx]);

                let mapped = bindings::dma_map_sg_attrs(
                    dev,
                    &mut (*pdu).sg[sg_idx],
                    nents,
                    dir,
                    bindings::DMA_ATTR_NO_WARN as u64,
                );
                // pr_info!("dma_map_sg_attrs");

                let mut sg = (*pdu).sg[sg_idx];
                let mut dma_len: i32 = sg.dma_length as i32;
                let mut dma_addr = sg.dma_address;
                let offset = (dma_addr % CTRL_PAGE_SIZE as u64) as i32;
                cmd.rw.prp1 = dma_addr.into();
                // pr_info!("set up prp1 {:#x?}", dma_addr);

                (*pdu).nents = mapped as i32;
                length -= CTRL_PAGE_SIZE - offset;

                if length > 0 {
                    dma_len -= CTRL_PAGE_SIZE - offset;
                    if dma_len > 0 {
                        dma_addr += (CTRL_PAGE_SIZE - offset) as u64;
                    } else {
                        sg_idx += 1;
                        sg = (*pdu).sg[sg_idx];
                        dma_len = sg.dma_length as i32;
                        dma_addr = sg.dma_address;
                    }
                }

                if length > CTRL_PAGE_SIZE {
                    let mut prp_dma: u64 = 0;
                    let l: *mut u64 = bindings::dma_alloc_attrs(
                        dev,
                        8 * MAX_SG,
                        &mut prp_dma,
                        bindings::___GFP_ATOMIC,
                        0,
                    ) as _;

                    // pr_info!("alloc coherent {:#x?}", l);

                    cmd.rw.prp2 = prp_dma.into();
                    for i in 0..MAX_SG {
                        *(l.add(i)) = dma_addr;
                        dma_len -= CTRL_PAGE_SIZE;
                        dma_addr += CTRL_PAGE_SIZE as u64;
                        length -= CTRL_PAGE_SIZE;

                        if length <= 0 {
                            break;
                        }
                        if dma_len > 0 {
                            continue;
                        }
                        sg_idx += 1;
                        sg = (*pdu).sg[sg_idx];
                        dma_len = sg.dma_length as i32;
                        dma_addr = sg.dma_address;
                    }
                } else {
                    pr_info!("no coherent {:#x?}", dma_addr);
                    cmd.rw.prp2 = dma_addr.into();
                }
            }

            (*queue).submit_command(&cmd, true);

            
            drop(queue);
            
            let mut a = Box::from_raw(
                (*(*req).q).queuedata as *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>>,
            );
            (*(*req).q).queuedata = Box::into_raw(a) as _;
        }

        return bindings::BLK_STS_OK as u8;
    }

    unsafe extern "C" fn complete(req: *mut bindings::request) {

        let pdu = rq_to_pdu(req);

        unsafe {
            let queue: *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>> =
                (*(*req).q).queuedata as *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>>;
            let mut dev: *mut bindings::device = (*queue).device;

        
            if (*pdu).nents > 0 {
                bindings::dma_unmap_sg_attrs(dev, &mut (*pdu).sg[0], (*pdu).nents, 2, 0);
            }

            let mut a = Box::from_raw(
                (*(*req).q).queuedata as *mut Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>>,
            );
            bindings::blk_mq_end_request(req, 0);

            (*(*req).q).queuedata = Box::into_raw(a) as _;
        }
    }
}
