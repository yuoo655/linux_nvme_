use core::{
    cell::UnsafeCell,
    clone::Clone,
    convert::TryInto,
    ffi::c_void,
    format_args,
    marker::PhantomData,
    ops::DerefMut,
    pin::Pin,
    sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering},
};

use kernel::{
    bindings,
    bindings::driver_init,
    blk::mq,
    c_str, define_pci_id_table,
    device::{self, Device, RawDevice},
    dma, driver,
    error::code::*,
    irq, pci,
    prelude::*,
    sync::{Arc, ArcBorrow, Mutex, SpinLock, UniqueArc},
    AtomicOptionalBoxedPtr,
};

pub mod nvme_defs;
pub mod nvme_impl;
pub mod nvme_mq;
pub mod nvme_queue;
pub mod nvme_traits;

pub use nvme_defs::*;
pub use nvme_impl::*;
pub use nvme_mq::*;
pub use nvme_queue::*;
pub use nvme_traits::*;



static mut MAJOR: i32 = 0;


pub struct NvmeData<A, T>
where
    A: NvmeTraits + 'static,
{
    pub admin_rq: RequestQueue,
    pub io_rq: RequestQueue,
    pub queues: SpinLock<NvmeQueues<A, T>>,
    pub bar: IoMem<8192, A>,
    pub db_stride: usize,
    pub admin_tag: Tag,
    pub io_tag: Tag,
}

pub struct NvmeIrqData {
    pub tag: Tag,
    pub queue: Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>>,
}

pub struct NvmeTraitsImpl {
    device: *mut bindings::device,
}

impl NvmeTraitsImpl {
    fn new(device: *mut bindings::device) -> Self {
        Self { device: device }
    }
}

impl NvmeTraits for NvmeTraitsImpl {
    fn dma_alloc(&self, size: usize, dma_handle: &mut u64) -> usize {
        let dev = self.device;
        let ret =
            unsafe { bindings::dma_alloc_attrs(dev, size, dma_handle, bindings::GFP_KERNEL, 0) };
        ret as usize
    }
    fn dma_dealloc(&self, cpu_addr: *mut (), dma_handle: u64, size: usize) {}

    fn ioremap(start: usize, size: usize) -> usize {
        let addr = unsafe { bindings::ioremap(start as u64, size as _) };
        if addr.is_null() {
            0
        } else {
            addr as usize
        }
    }
    fn iounmap(start: usize) {
        unsafe { bindings::iounmap(start as _) }
    }
    fn readl(offset: usize) -> u32 {
        let val = unsafe { bindings::readl(offset as _) };
        val
    }

    fn writel(val: u32, offset: usize) {
        unsafe { bindings::writel(val, offset as _) }
    }

    fn readq(offset: usize) -> u64 {
        let val = unsafe { bindings::readq(offset as _) };
        val
    }

    fn writeq(val: u64, offset: usize) {
        unsafe { bindings::writeq(val, offset as _) }
    }

    fn writew(val: u16, offset: usize) {
        unsafe { bindings::writew(val, offset as _) }
    }
}

pub struct NvmeQueues<A, T>
where
    A: NvmeTraits + 'static,
{
    phantom: PhantomData<A>,
    pub admin_queue: Option<Arc<NvmeQueue<A, T>>>,
    pub io_queues: Vec<Arc<NvmeQueue<A, T>>>,
}

impl<A, T> NvmeQueues<A, T>
where
    A: NvmeTraits,
{
    pub fn new() -> Self {
        Self {
            phantom: PhantomData,
            admin_queue: None,
            io_queues: Vec::new(),
        }
    }
}



pub struct NvmeDeviceData{
    

    admin_irq: irq::Registration<NvmeDevice>,
    io_irq: irq::Registration<NvmeDevice>,
}


type DeviceData = device::Data<(), (), Arc<NvmeDeviceData>>;

struct NvmeDevice;

impl NvmeDevice {}

impl pci::Driver for NvmeDevice {
    type Data = Arc<DeviceData>;

    define_pci_id_table! {
        (),
        [ (pci::DeviceId::with_class(bindings::PCI_CLASS_STORAGE_EXPRESS, 0xffffff), None) ]
    }

    fn probe(dev: &mut pci::Device, _id: Option<&Self::IdInfo>) -> Result<Self::Data> {

       
        
        let bars = dev.select_bars(bindings::IORESOURCE_MEM.into());
        dev.request_selected_regions(bars, c_str!("nvme"))?;
        dev.enable_device_mem()?;
        dev.set_master();
        dev.alloc_irq_vectors(2, 2, bindings::PCI_IRQ_ALL_TYPES)?;
        dev.dma_set_mask(!0)?;
        dev.dma_set_coherent_mask(!0)?;

        let res = dev.take_resource(0).ok_or(ENXIO)?;
        let iomem_offset = res.offset;
        let iomem_size = res.size as usize;
        let bar = IoMem::<8192, NvmeTraitsImpl>::new(iomem_offset as usize, iomem_size);

        let bar_ptr = bar.ptr;

        let mut nvme_queues = NvmeQueues::<NvmeTraitsImpl, *mut bindings::device>::new();
        let mut admin_tag = Tag::new::<BlkAdminOps>();
        let mut admin_rq = RequestQueue::new(&mut admin_tag);
        let mut io_tag = Tag::new::<BlkIoOps>();
        unsafe {
            io_tag.set.nr_hw_queues = 1;
            io_tag.set.queue_depth = NVME_QUEUE_DEPTH as u32;
            io_tag.set.timeout = 30 * bindings::HZ;
            io_tag.set.numa_node = -1;
            io_tag.set.cmd_size = core::mem::size_of::<Pdu>() as u32;
            let _r = bindings::blk_mq_alloc_tag_set(&mut io_tag.set);
        }
        let mut io_rq = RequestQueue::new(&mut io_tag);

        let nvme_data = NvmeData{
            queues: unsafe { SpinLock::new(nvme_queues) },
            admin_rq: admin_rq,
            io_rq: io_rq,
            bar: bar,
            db_stride: 0,
            admin_tag,
            io_tag,            
        };

        let uniquearc = UniqueArc::try_new(nvme_data).unwrap();
        let mut data = Pin::from(uniquearc);
        let queues = unsafe { data.as_mut().map_unchecked_mut(|d| &mut d.queues) };
        kernel::spinlock_init!(queues, "DeviceData::queues");

        let mut nvme_data: Arc<NvmeData<NvmeTraitsImpl, *mut bindings::device>> =
            Arc::<NvmeData<NvmeTraitsImpl, *mut bindings::device>>::from(data);

        let raw_dev = dev.raw_device();
        let nvme_dev = NvmeTraitsImpl::new(raw_dev);

        let admin_queue = NvmeQueue::<NvmeTraitsImpl, *mut bindings::device>::new(
            nvme_dev,
            raw_dev,
            nvme_data.clone(),
            0,
            (NVME_QUEUE_DEPTH ) as u16,
            0,
            false,
            0,
        );






        let nvme_dev = NvmeTraitsImpl::new(raw_dev);

        let io_queue = NvmeQueue::<NvmeTraitsImpl, *mut bindings::device>::new(
            nvme_dev,
            raw_dev,
            nvme_data.clone(),
            1,
            (NVME_QUEUE_DEPTH)as u16,
            1,
            false,
            0x4,
        );

        let admin_queue: Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>> = Pin::from(UniqueArc::try_new(admin_queue).unwrap()).into();
        let io_queue: Arc<NvmeQueue<NvmeTraitsImpl, *mut bindings::device>> = Pin::from(UniqueArc::try_new(io_queue).unwrap()).into();


        unsafe {
            (*nvme_data.admin_rq.ptr).queuedata =
                Box::into_raw(Box::try_new(admin_queue.clone()).unwrap()) as _;
        }

        unsafe {
            (*nvme_data.io_rq.ptr).queuedata =
                Box::into_raw(Box::try_new(io_queue.clone()).unwrap()) as _;
        }


        let mut admin_irq_data = unsafe {
            SpinLock::new(
                NvmeIrqData{
                    tag: admin_tag,
                    queue: admin_queue.clone()
                }
            )
        };
        kernel::spinlock_init!(
            unsafe { Pin::new_unchecked(&mut admin_irq_data) },
            "nvme_admin_irq"
        );
        let ret = unsafe { bindings::pci_irq_vector(dev.ptr, 0) };
        let admin_irq = irq::Registration::<NvmeDevice>::try_new(
            ret as _,
            Arc::try_new(admin_irq_data).unwrap(),
            irq::flags::SHARED,
            fmt!("nvme admin irq"),
        )
        .unwrap();


        let mut io_irq_data = unsafe {
            SpinLock::new(
                NvmeIrqData{
                    tag: io_tag,
                    queue: io_queue.clone()
                }
            )
        };
        kernel::spinlock_init!(
            unsafe { Pin::new_unchecked(&mut io_irq_data) },
            "nvme_io_irq"
        );
        let ret = unsafe { bindings::pci_irq_vector(dev.ptr, 1) };
        let io_irq = irq::Registration::<NvmeDevice>::try_new(
            ret as _,
            Arc::try_new(io_irq_data).unwrap(),
            irq::flags::SHARED,
            fmt!("nvme io irq"),
        )
        .unwrap();




        // config admin queue
        config_admin_queue(&nvme_data, &admin_queue);
        // setup io queue
        set_queue_count(1, &nvme_data);
        alloc_completion_queue(&nvme_data, &io_queue);
        alloc_submission_queue(&nvme_data, &io_queue);


        nvme_data.queues.lock().admin_queue = Some(admin_queue.clone());
        nvme_data.queues.lock().io_queues.try_push(io_queue.clone());


        let mut disks = Vec::try_with_capacity(1).unwrap();


        let size = core::mem::size_of::<u8>() * 8192;
        let mut id_dma_handle = 0 as u64;
        let id_cpu_addr = unsafe {
            bindings::dma_alloc_attrs(
                dev.raw_device(),
                size,
                &mut id_dma_handle,
                bindings::GFP_KERNEL,
                0,
            )
        };
        
        let mut rt_dma_handle = 0 as u64;
        let rt_cpu_addr = unsafe {
            bindings::dma_alloc_attrs(
                dev.raw_device(),
                size,
                &mut rt_dma_handle,
                bindings::GFP_KERNEL,
                0,
            )
        };


        identify(&nvme_data, 0, 1, id_dma_handle);


        let nn = 1;
        for i in 1..=nn {
            pr_info!(" number_of_namespaces i {:?}", i);

            identify(&nvme_data, i, 0, id_dma_handle);

            let id_ns = unsafe { &*(id_cpu_addr as *const NvmeIdNs) };
            if id_ns.ncap.into() == 0 {
                continue;
            }

            get_features(&nvme_data, FEAT_LBA_RANGE, i, rt_dma_handle);

            let _rt = unsafe { &*(rt_cpu_addr as *const NvmeLbaRangeType) };

            let io_queue_ptr = Box::into_raw(Box::try_new(io_queue.clone()).unwrap()) as _;

            let mut disk = unsafe {
                bindings::__blk_mq_alloc_disk(&mut io_tag.set, io_queue_ptr, core::ptr::null_mut())
            };

            //alloc ns
            let lbaf = (id_ns.flbas & 0xf) as usize;
            let lba_shift = id_ns.lbaf[lbaf].ds as u32;
            let ns = Box::try_new(NvmeNamespace {
                id: i,
                lba_shift: 9,
            })
            .unwrap();

            let capacity = id_ns.nsze.into() << (lba_shift - bindings::SECTOR_SHIFT);

            const MAX_SG: usize = 16;
            unsafe {
                bindings::blk_queue_max_segments((*disk).queue, MAX_SG as u16);
                (*disk).major = MAJOR;
                (*disk).minors = 64;
                let name = c_str!("nvme0n1");
                for i in 0..name.len() {
                    (*disk).disk_name[i] = name[i] as i8;
                }
                (*disk).fops = &BlkDevOps::OPS;
                bindings::set_capacity(disk, capacity);
            }
            disks.try_push(disk).unwrap();
        }


        for d in disks {
            pr_info!("add disk");
            unsafe {
                bindings::device_add_disk(&mut (*dev.ptr).dev, d, core::ptr::null_mut())
            };
        }


        
        
        let info = Arc::try_new(NvmeDeviceData {
            admin_irq,
            io_irq,
        })?;


        let mut data = Pin::from(kernel::new_device_data!((), (), info.clone(), "Nvme::Data")?);
        let mut data = Arc::<DeviceData>::from(data);

        Ok(data)
    }

    fn remove(_data: &Self::Data) {
        todo!()
    }

}

fn config_admin_queue(nvme_data: &Arc<NvmeData<NvmeTraitsImpl, *mut bindings::device>>, admin_queue: &NvmeQueue<NvmeTraitsImpl, *mut bindings::device>) {
    let bar = &nvme_data.bar;
    bar.writel(0, OFFSET_CC);
    wait_idle(nvme_data);

    let mut aqa = (NVME_QUEUE_DEPTH - 1) as u32;
    aqa |= aqa << 16;
    let mut ctrl_config = NVME_CC_ENABLE | NVME_CC_CSS_NVM;
    ctrl_config |= 0 << NVME_CC_MPS_SHIFT;
    ctrl_config |= NVME_CC_ARB_RR | NVME_CC_SHN_NONE;
    ctrl_config |= NVME_CC_IOSQES | NVME_CC_IOCQES;
    {
        bar.writel(aqa, OFFSET_AQA);
        bar.writeq(admin_queue.sq.dma_handle, OFFSET_ASQ);
        bar.writeq(admin_queue.cq.dma_handle, OFFSET_ACQ);
        bar.writel(ctrl_config, OFFSET_CC);
    }
}

struct NvmeModule {
    driver: Pin<Box<driver::Registration<pci::Adapter<NvmeDevice>>>>,
}

impl kernel::Module for NvmeModule {
    fn init(_name: &'static CStr, module: &'static ThisModule) -> Result<Self> {
        unsafe {
            MAJOR = bindings::__register_blkdev(0, c_str!("nvme").as_char_ptr(), None);
        }
        let driver = driver::Registration::new_pinned(c_str!("nvme"), module)?;

        Ok(NvmeModule { driver: driver })
    }
}

module! {
    type: NvmeModule,
    name: "nvme",
    author: "cl",
    license: "GPL v2",
}




fn submit_sync_command<A:NvmeTraits, T> (
    nvme_dev: &Arc<NvmeData<A, T>>,
    mut cmd: NvmeCommand,
) {


    let nvme = nvme_dev;
    let admin_rq = &nvme.admin_rq;
    let rw = bindings::req_op_REQ_OP_DRV_IN;

    unsafe{
        let rq = bindings::blk_mq_alloc_request(admin_rq.ptr, rw, 0);
        let err = bindings::IS_ERR(rq as *const c_void);
        let mut pdu = rq_to_pdu(rq);
        let mut cmnd =  (*pdu).cmnd;

        (*pdu).cmnd.common.opcode = cmd.common.opcode;
        (*pdu).cmnd.common.flags = cmd.common.flags;
        (*pdu).cmnd.common.command_id = (*rq).tag as u16;
        (*pdu).cmnd.common.nsid = cmd.common.nsid;
        (*pdu).cmnd.common.cdw2 = cmd.common.cdw2;
        (*pdu).cmnd.common.metadata = cmd.common.metadata;
        (*pdu).cmnd.common.prp1 = cmd.common.prp1;
        (*pdu).cmnd.common.prp2 = cmd.common.prp2;
        (*pdu).cmnd.common.cdw10 = cmd.common.cdw10;

        let _status = bindings::blk_execute_rq(rq, false);


        let pdu = rq_to_pdu(rq);
        let result = (*pdu).result;
        pr_info!("blk_mq_free_request");
        bindings::blk_mq_free_request(rq);
    }
}



impl irq::Handler for NvmeDevice
{
    type Data = Arc<SpinLock<NvmeIrqData>>;

    fn handle_irq(irq_data: ArcBorrow<'_, SpinLock<NvmeIrqData>>) -> irq::Return {
        pr_info!("nvme irq");

        let data = irq_data.lock();
        let queue = &data.queue;

        let qid = queue.qid;

        let tag = data.tag;
        let bar = &queue.device_data.bar;

        let mut head = queue.cq_head.load(Ordering::Relaxed);
        let mut phase = queue.cq_phase.load(Ordering::Relaxed);
        let mut found = 0;

        let _head = head as usize;
        pr_info!("--------------------------head {:?}", _head as usize);

        
        
        let maps = unsafe{core::slice::from_raw_parts_mut(tag.set.tags, 1)};
        
        // queue.cq_head.store(head, Ordering::Relaxed);
        // queue.cq_phase.store(phase, Ordering::Relaxed);
        // unsafe{
        //     if queue.nvme_cqe_pending(){
        //         let cqe: NvmeCompletion = queue.cq.read_volatile(_head).unwrap();
        //         pr_info!("command_id {:?} result {:?} status {:?}", cqe.command_id as u32, cqe.result.into() as usize, cqe.status.into() as usize);

        //         let rq = bindings::blk_mq_tag_to_rq(maps[0], cqe.command_id as u32);
        //         if !rq.is_null() {
        //             let pdu = rq_to_pdu(rq);
        //             (*pdu).result = cqe.result.into();
        //             (*pdu).status = cqe.status.into() >> 1;
        //             bindings::blk_mq_complete_request(rq);
        //         };
        //         queue.nvme_update_cq_head();
        //         queue.nvme_ring_cq_doorbell();
        //         irq::Return::Handled
        //     }else{
        //         irq::Return::None
        //     }
        // }

        
        
        loop {
            let cqe = queue.cq.read_volatile(head.into()).unwrap();
            if cqe.status.into() & 1 != phase {
                break;
            }
            let cqe = queue.cq.read_volatile(head.into()).unwrap();

            pr_info!("------id {:?} head {:?} phase {:?} result {:?} status {:?}", cqe.command_id as u32, head as usize, phase  as usize, cqe.result.into() as usize, cqe.status.into() as usize);
            unsafe{
                let rq = bindings::blk_mq_tag_to_rq(maps[0], cqe.command_id as u32);
                if !rq.is_null() {
                    let pdu = rq_to_pdu(rq);
                    (*pdu).result = cqe.result.into();
                    (*pdu).status = cqe.status.into() >> 1;
                    bindings::blk_mq_complete_request(rq);
                };
            }   
            found += 1;
            head += 1;
            if head == queue.q_depth {
                head = 0;
                phase ^= 1;
            }            
        }

        queue.cq_head.store(head, Ordering::Relaxed);
        queue.cq_phase.store(phase, Ordering::Relaxed);

        bar.writel(head.into(), queue.db_offset + 0x4);
        drop(data);
        irq::Return::Handled

        // if found == 0 {
        //     // drop(data);
        //     irq::Return::None
        // }else{
        //     // drop(data);
        // }
    }
}


