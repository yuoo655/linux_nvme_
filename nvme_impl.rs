use crate::nvme_defs::*;
use crate::nvme_traits::*;
use crate::*;
// use crate::submit_sync_command;

use kernel::prelude::*;

pub fn wait_ready<A: NvmeTraits, T>(nvme_data: &Arc<NvmeData<A, T>>) {
    pr_info!("Waiting for controller ready\n");
    let bar = &nvme_data.bar;
    while bar.readl(OFFSET_CSTS) & NVME_CSTS_RDY == 0 {}
    pr_info!("Controller ready\n");
}


pub fn wait_idle<A: NvmeTraits, T>(nvme_data: &Arc<NvmeData<A, T>>) {
    pr_info!("Waiting for controller idle\n");

    let bar = &nvme_data.bar;
    while bar.readl(OFFSET_CSTS) & NVME_CSTS_RDY != 0 {}

    pr_info!("Controller ready\n");
}


fn config_admin_queue<A: NvmeTraits, T>(nvme_data: &Arc<NvmeData<A, T>>, admin_queue: &NvmeQueue<A, T>) {

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
    wait_ready(&nvme_data);
}


pub fn alloc_completion_queue<A: NvmeTraits, T>(
    nvme_data: &Arc<NvmeData<A, T>>,
    queue: &NvmeQueue<A, T>
) {
    pr_info!("alloc cq");
    let mut flags = NVME_QUEUE_PHYS_CONTIG;
    if !queue.polled {
        flags |= NVME_CQ_IRQ_ENABLED;
    }

    let cmd = NvmeCommand {
        create_cq: NvmeCreateCq {
            opcode: NvmeAdminOpcode::create_cq as _,
            prp1: queue.cq.dma_handle.into(),
            cqid: queue.qid.into(),
            qsize: (queue.q_depth - 1).into(),
            cq_flags: flags.into(),
            irq_vector: queue.cq_vector.into(),
            ..NvmeCreateCq::default()
        },
    };
    submit_sync_command(nvme_data, cmd);
}

pub fn alloc_submission_queue<A: NvmeTraits, T>(
    nvme_data: &Arc<NvmeData<A, T>>,
    queue: &NvmeQueue<A, T>
) {
    pr_info!("alloc sq");
    let cmd = NvmeCommand {
        create_sq: NvmeCreateSq {
            opcode: NvmeAdminOpcode::create_sq as _,
            prp1: queue.sq.dma_handle.into(),
            sqid: queue.qid.into(),
            qsize: (queue.q_depth - 1).into(),
            sq_flags: (NVME_QUEUE_PHYS_CONTIG | NVME_SQ_PRIO_MEDIUM).into(),
            cqid: queue.qid.into(),
            ..NvmeCreateSq::default()
        },
    };

    submit_sync_command(nvme_data, cmd);
}

pub fn set_queue_count<A: NvmeTraits, T>(
    count: u32,
    nvme_data: &Arc<NvmeData<A, T>>,
) {
    pr_info!("set queue count");
    let q_count = count;
    set_features(NVME_FEAT_NUM_QUEUES, q_count, 0, nvme_data);
}

pub fn set_features<A: NvmeTraits, T>(
    fid: u32,
    dword11: u32,
    dma_addr: u64,
    nvme_data: &Arc<NvmeData<A, T>>,
) {
    pr_info!("fid: {}, dma: {}, dword11: {}", fid, dma_addr, dword11);

    submit_sync_command(
        nvme_data,
        NvmeCommand {
            features: NvmeFeatures {
                opcode: NvmeAdminOpcode::set_features as _,
                // prp1: dma_addr.into(),
                fid: fid.into(),
                dword11: dword11.into(),
                ..NvmeFeatures::default()
            },
        },
    );
    pr_info!("Set features done!");
}

pub fn identify<A: NvmeTraits, T>(
    nvme_data: &Arc<NvmeData<A, T>>,
    nsid: u32,
    cns: u32,
    dma_addr: u64,
) {
    pr_info!("nvme identify");
    submit_sync_command(
        nvme_data,
        NvmeCommand {
            identify: NvmeIdentify {
                opcode: NvmeAdminOpcode::identify as _,
                nsid: nsid.into(),
                prp1: dma_addr.into(),
                cns: cns.into(),
                ..NvmeIdentify::default()
            },
        },
    )
}

pub fn get_features<A: NvmeTraits, T>(
    nvme_data: &Arc<NvmeData<A, T>>,
    fid: u32,
    nsid: u32,
    dma_addr: u64,
) {
    pr_info!("nvme get_features");
    submit_sync_command(
        nvme_data,
        NvmeCommand {
            features: NvmeFeatures {
                opcode: NvmeAdminOpcode::get_features as _,
                nsid: nsid.into(),
                prp1: dma_addr.into(),
                fid: fid.into(),
                ..NvmeFeatures::default()
            },
        },
    )
}




