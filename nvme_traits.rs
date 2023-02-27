use core;
use core::marker::PhantomData;

pub trait NvmeTraits {
    fn dma_alloc(&self, size: usize, dma_handle: &mut u64) -> usize {
        0
    }
    fn dma_dealloc(&self, cpu_addr: *mut (), dma_handle: u64, size: usize) {
        
    }
    fn ioremap(start: usize, size: usize) -> usize {
        start
    }
    fn iounmap(start: usize) {}

    fn writew(val: u16, offset: usize) {}

    fn readl(offset: usize) -> u32 {
        0
    }

    fn writel(val: u32, offset: usize) {}

    fn readq(offset: usize) -> u64 {
        0
    }
    fn writeq(val: u64, offset: usize) {}
}

pub fn dma_alloc<T, A: NvmeTraits>(count: usize, dev: A) -> DmaInfo<T, A> {
    let t_size = core::mem::size_of::<T>();
    let size = count.checked_mul(t_size).unwrap();
    let mut dma_handle = 0;
    let cpu_addr = dev.dma_alloc(size, &mut dma_handle);
    DmaInfo::new(cpu_addr as _, dma_handle, count)
}

pub struct DmaInfo<T, A: NvmeTraits> {
    dma: PhantomData<A>,
    pub count: usize,
    pub dma_handle: u64,
    pub cpu_addr: *mut T,
}

impl<T, A: NvmeTraits> DmaInfo<T, A> {
    pub fn new(cpu_addr: *mut T, dma_handle: u64, count: usize) -> Self {
        Self {
            count: count,
            dma_handle: dma_handle,
            cpu_addr: cpu_addr,
            dma: PhantomData,
        }
    }

    pub fn read_volatile(&self, index: usize) -> Option<T> {
        if index >= self.count {
            // pr_info!("read_volatile index:{:?} count:{:?}", index, self.count);
            return None;
        }

        let ptr = self.cpu_addr.wrapping_add(index);

        // SAFETY: We just checked that the index is within bounds.
        Some(unsafe { ptr.read() })
    }

    pub fn write_volatile(&self, index: usize, value: &T) -> bool
    where
        T: Copy,
    {
        if index >= self.count {
            // pr_info!("read_volatile index:{:?} count:{:?}", index, self.count);
            return false;
        }

        let ptr = self.cpu_addr.wrapping_add(index);

        // pr_info!("write_volatile");
        // SAFETY: We just checked that the index is within bounds.
        unsafe { ptr.write(*value) };
        true
    }

    pub fn first_ptr(&self) -> *const T {
        self.cpu_addr
    }
}

pub struct IoMem<const SIZE: usize, A: NvmeTraits> {
    iomapper: PhantomData<A>,
    pub ptr: usize,
}

impl<const SIZE: usize, A: NvmeTraits> IoMem<SIZE, A> {
    pub fn new(start: usize, size: usize) -> Self {
        let addr = A::ioremap(start, size);
        Self {
            iomapper: PhantomData,
            ptr: addr as usize,
        }
    }

    pub fn readl(&self, offset: usize) -> u32 {
        let val = A::readl(self.ptr + offset);
        val
    }

    pub fn readq(&self, offset: usize) -> u64 {
        let val = A::readq(self.ptr + offset);
        val
    }

    pub fn writew(&self, val: u16, offset: usize) {
        A::writew(val, self.ptr + offset);
    }

    pub fn writel(&self, val: u32, offset: usize) {
        A::writel(val, self.ptr + offset);
    }

    pub fn writeq(&self, val: u64, offset: usize) {
        A::writeq(val, self.ptr + offset);
    }
}
