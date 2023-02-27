use core::marker::PhantomData;
use core::sync::atomic::AtomicU16;
use core::sync::atomic::Ordering;

use kernel::sync::Arc;
use kernel::prelude::Vec;
use kernel::pr_info;





use crate::nvme_impl::*;
use crate::nvme_defs::*;
use crate::nvme_traits::*;
use crate::*;



pub struct NvmeQueueInner {
    pub sq_tail: AtomicU16,
    pub last_sq_tail: AtomicU16,
}

impl NvmeQueueInner {
    pub fn new() -> Self {
        Self {
            sq_tail: AtomicU16::new(0),
            last_sq_tail: AtomicU16::new(0),
        }
    }
}


pub struct NvmeQueue<A, T> 
where
    A: NvmeTraits + 'static
{

    pub device_data: Arc<NvmeData<A, T>>, 

    pub device: T,

    pub db_offset: usize,
    pub sdb_index: usize,
    pub qid: u16,
    pub polled: bool,

    pub cq_head: AtomicU16,
    pub cq_phase: AtomicU16,

    pub sq: DmaInfo<NvmeCommand, A>,
    pub cq: DmaInfo<NvmeCompletion, A>,

    pub q_depth: u16,
    pub cq_vector: u16,

    pub inner: NvmeQueueInner,

}

impl<A, T: 'static> NvmeQueue<A, T>
where
    A: NvmeTraits,
{
    pub fn new(dev: A, device: T, dev_data: Arc<NvmeData<A, T>>, qid: u16, depth: u16, vector: u16, polled: bool, db_stride: usize) -> Self {


        let t_size = core::mem::size_of::<NvmeCompletion>();
        let size = (depth as usize).checked_mul(t_size).unwrap();
        let mut dma_handle = 0;
        let cpu_addr = dev.dma_alloc(size, &mut dma_handle);
        let cq = DmaInfo::<NvmeCompletion, A>::new(cpu_addr as _, dma_handle, depth.into());


        let t_size = core::mem::size_of::<NvmeCommand>();
        let size = (depth as usize).checked_mul(t_size).unwrap();
        let mut dma_handle = 0;
        let cpu_addr = dev.dma_alloc(size, &mut dma_handle);
        let sq = DmaInfo::<NvmeCommand, A>::new(cpu_addr as _, dma_handle, depth.into());
        // let cq: DmaInfo<NvmeCompletion, A> = dma_alloc::<NvmeCompletion, A>(depth.into(), dev);
        // let sq: DmaInfo<NvmeCommand, A> = dma_alloc::<NvmeCommand, A>(depth.into(), dev);


        for i in 0..depth {
            cq.write_volatile(i.into(), &NvmeCompletion::default());
        }

        let queue_inner = NvmeQueueInner::new();

        let sdb_offset = (qid as usize) * db_stride * 2;
        let db_offset = sdb_offset + 4096;

        Self {
            device_data: dev_data,
            device: device,
            db_offset,
            sdb_index: sdb_offset / 4,
            qid,
            sq,
            cq,
            q_depth: depth,
            cq_vector: vector,
            cq_head: AtomicU16::new(0),
            cq_phase: AtomicU16::new(1),
            inner: queue_inner,
            polled,
        }
    }

    pub fn submit_command(&self, cmd: &NvmeCommand, is_last: bool) {

        let mut sq_tail = self.inner.sq_tail.load(Ordering::Relaxed);

        self.sq.write_volatile(sq_tail.into(), cmd);
        pr_info!("----------------------------tail {:?}", sq_tail);
        sq_tail += 1;
        if sq_tail == self.q_depth {
           sq_tail = 0;
        }
        self.inner.sq_tail.store(sq_tail, Ordering::Relaxed);

        self.nvme_write_sq_db(is_last);
    }

    // write submission queue doorbell to notify nvme device
    pub fn nvme_write_sq_db(&self, write_sq: bool) {

        let bar = &self.device_data.bar;

        let mut sq_tail = self.inner.sq_tail.load(Ordering::Relaxed);
        let mut last_sq_tail = self.inner.last_sq_tail.load(Ordering::Relaxed);

        if !write_sq {
            let mut next_tail = sq_tail + 1;
            if next_tail == self.q_depth {
                next_tail = 0;
            }
            if next_tail != last_sq_tail {
                return;
            }
        }

        bar.writel(sq_tail.into(), self.db_offset);
        last_sq_tail = sq_tail;

        self.inner.sq_tail.store(sq_tail, Ordering::Relaxed);
        self.inner.last_sq_tail.store(last_sq_tail, Ordering::Relaxed);

    }

    // check if there is completed command in completion queue
    pub fn nvme_cqe_pending(&self) -> bool {

        let head = self.cq_head.load(Ordering::Relaxed);
        let phase = self.cq_phase.load(Ordering::Relaxed);
        let cqe = self.cq.read_volatile(head.into()).unwrap();

        if cqe.status.into() & 1 != phase {
            return true;
        } else {
            return false;
        }
    }

    // update completion queue head
    pub fn nvme_update_cq_head(&self) {
        let mut head = self.cq_head.load(Ordering::Relaxed);
        let mut phase = self.cq_phase.load(Ordering::Relaxed);

        let next_head = head + 1;
        if next_head == self.q_depth {
            head = 0;
            phase ^= 1;
        } else {
            head = next_head;
        }
    }

    // notify nvme device we've completed the command
    pub fn nvme_ring_cq_doorbell(&self) {
        let bar = &self.device_data.bar;
        let head = self.cq_head.load(Ordering::Relaxed);
        bar.writel(head.into(), self.db_offset + 0x4);
    }

    // check completion queue and update cq head cq doorbell until there is no pending command
    pub fn nvme_poll_cq(&self) {
        
        if !self.nvme_cqe_pending() {}
        self.nvme_update_cq_head();
        self.nvme_ring_cq_doorbell();
    }

    /// Processes the completion queue.
    ///
    /// Returns `true` if at least one entry was processed, `false` otherwise.
    pub fn process_completions(&self) -> i32 {
        let bar = &self.device_data.bar;

        let mut head = self.cq_head.load(Ordering::Relaxed);
        let mut phase = self.cq_phase.load(Ordering::Relaxed);
        let mut found = 0;

        loop {
            let cqe = self.cq.read_volatile(head.into()).unwrap();

            if cqe.status.into() & 1 != phase {
                break;
            }

            let cqe = self.cq.read_volatile(head.into()).unwrap();

            found += 1;
            head += 1;
            if head == self.q_depth {
                head = 0;
                phase ^= 1;
            }
        }

        if found == 0 {
            return found;
        }

        bar.writel(head.into(), self.db_offset + 0x4);

        self.cq_head.store(head, Ordering::Relaxed);
        self.cq_phase.store(phase, Ordering::Relaxed);

        found
    }



    // pub fn register_irq(self: &Arc<Self>, pci_dev: &pci::Device) -> Result {
    //     pr_info!(
    //         "Registering irq for queue qid: {}, vector {}\n",
    //         self.qid,
    //         self.cq_vector
    //     );
    //     let irq = pci_dev.request_irq::<Self>(
    //         self.cq_vector.into(),
    //         self.clone(),
    //         format_args!("nvme{}q{}", 0, self.qid),
    //     )?;
    //     Ok(())
    // }
}



// impl<A, T: 'static> irq::Handler for NvmeQueue<A, T>
// where
//     A: NvmeTraits + 'static,
// {
//     type Data = Arc<NvmeQueue<A, T>>;

//     fn handle_irq(queue: ArcBorrow<'_, NvmeQueue<A, T>>) -> irq::Return {
//         pr_info!("nvme irq");

//         // queue.nvme_poll_cq();
        
//         irq::Return::Handled
//         // if queue.process_completions() != 0 {
//         //     irq::Return::Handled
//         // } else {
//         //     irq::Return::None
//         // }
//     }
// }