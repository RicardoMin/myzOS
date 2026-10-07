use super::{PAGE_SIZE, RAM_END, RAM_START};

use crate::spinlock::SpinLock;


static KERNEL_POOL: SpinLock<Option<PagePool>> =
    SpinLock::new(None);
static DATA_POOL: SpinLock<Option<PagePool>> =
    SpinLock::new(None);

pub const KERNEL_PAGES: usize = 1024;

#[cfg(feature = "lab2-tests")]
pub fn check_all_free() {
    let kernel_slot = KERNEL_POOL.lock();
    let data_slot = DATA_POOL.lock();

    let kernel_pool = kernel_slot
        .as_ref()
        .expect("kernel pool not initialized");
    let data_pool = data_slot
        .as_ref()
        .expect("data pool not initialized");

    kernel_pool.check_initial_chain();
    data_pool.check_initial_chain();
}

#[repr(C)]
struct PageNode {
    next: usize,
}

pub struct PagePool {
    begin: usize,
    end: usize,
    head: usize,
    free_count: usize,
}


impl PagePool {
    /// # Safety
    /// [begin, end) 必须是当前可读写、尚未被占用的物理内存
    /// 构造后由这个页池独占管理；不得重复初始化重叠范围
    /// 当前地址访问方式必须允许直接使用这些物理地址
    pub unsafe fn new(begin: usize, end: usize) -> Self {
        assert!(RAM_START <= begin);
        assert!(begin < end);
        assert!(end <= RAM_END);
        assert!(begin % PAGE_SIZE == 0);
        assert!(end % PAGE_SIZE == 0);

        let mut pool = Self {
            begin,
            end,
            head: 0,
            free_count: 0,
        };

        let mut pa = begin;

        while pa < end {
            unsafe {
                (pa as *mut PageNode).write(PageNode {
                    next: pool.head,
                });
            }

            pool.head = pa;
            pool.free_count += 1;

            pa += PAGE_SIZE;
        }

        pool
    }

    pub fn free_pages(&self) -> usize {
        self.free_count
    }

    pub fn capacity(&self) -> usize {
        (self.end - self.begin) / PAGE_SIZE
    }

    pub fn try_alloc(&mut self) -> Option<usize> {
        if self.head == 0 {
            assert_eq!(self.free_count, 0);
            return None;
        }
        assert!(self.free_count > 0);

        let pa = self.head;
        self.assert_page_in_pool(pa);

        let next = unsafe {
            (pa as *const PageNode).read().next
        };
        if next != 0 {
            self.assert_page_in_pool(next);
        }

        self.head = next;
        self.free_count -= 1;

        unsafe {
            core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
        }
        Some(pa)
    }

    pub fn alloc(&mut self) -> usize {
        self.try_alloc().expect("page pool exhausted")
    }

    /// # Safety
    /// pa 必须是从本池申请、且尚未归还的一页
    /// 归还时，所有使用者都已停止访问该页
    /// 归还后，不得继续通过旧地址或引用读写该页
    pub unsafe fn free(&mut self, pa: usize) {
        self.assert_page_in_pool(pa);
        assert!(self.free_count < self.capacity());

        unsafe {
            (pa as *mut PageNode).write(PageNode {
                next: self.head
            });
        }

        self.head = pa;
        self.free_count += 1;
    }

    fn assert_page_in_pool(&self, pa: usize) {
        assert!(self.begin <= pa && pa < self.end);
        assert_eq!(pa % PAGE_SIZE, 0);
    }

    pub fn check_free_chain(&self) {
        let capacity = self.capacity();
        let mut cursor = self.head;
        let mut seen = 0;

        while cursor != 0 {
            self.assert_page_in_pool(cursor);
            assert!(seen < capacity);

            cursor = unsafe {
                (cursor as *const PageNode).read().next
            };
            seen += 1;
        }

        assert_eq!(seen, self.free_count);
    }

    pub fn check_initial_chain(&self) {
        self.check_free_chain();
        assert_eq!(self.free_count, self.capacity());
    }
}


pub fn init(kernel: PagePool, data: PagePool) {
    let mut kernel_guard = KERNEL_POOL.lock();
    let mut data_guard = DATA_POOL.lock();

    assert!(kernel_guard.is_none());
    assert!(data_guard.is_none());

    *kernel_guard = Some(kernel);
    *data_guard = Some(data);
}

pub fn alloc_kernel() -> usize {
    let mut slot = KERNEL_POOL.lock();

    let pool = slot
        .as_mut()
        .expect("kernel pool not initialized");

    pool.alloc()
}

pub fn alloc_data() -> usize {
    let mut slot = DATA_POOL.lock();

    let pool = slot 
        .as_mut()
        .expect("data pool not initialized");

    pool.alloc()
}

/// # Safety
/// 必须归还到原来的池；
/// 页面尚未归还，所有访问已经停止
pub unsafe fn free_kernel(pa: usize) {
    let mut slot = KERNEL_POOL.lock();

    let pool = slot
        .as_mut()
        .expect("kernel pool not initialized");

    unsafe {
        pool.free(pa);
    }
}

/// #SAFETY
/// 必须归还到原来的池
/// 页面尚未归还，所有访问已经停止
pub unsafe fn free_data(pa: usize) {
    let mut slot = DATA_POOL.lock();

    let pool = slot
        .as_mut()
        .expect("data pool not initialized");

    unsafe {
        pool.free(pa);
    }
}

pub fn free_counts() -> (usize, usize) {
    let slot1 = KERNEL_POOL.lock();
    let slot2 = DATA_POOL.lock();

    let pool1 = slot1
        .as_ref()
        .expect("kernel pool not initialized");
    let pool2 = slot2
        .as_ref()
        .expect("data pool not initialized");

    (pool1.free_pages(), pool2.free_pages())
}