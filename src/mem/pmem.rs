use super::{PAGE_SIZE, RAM_END, RAM_START};

pub const KERNEL_PAGES: usize = 1024;


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