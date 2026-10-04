use core::sync::atomic::{AtomicBool, Ordering::{self, Release}};

use crate::{cpu, console, mem::pmem::free_counts, spinlock::SpinLock};
use super::{pmem, PAGE_SIZE};

static HELD: SpinLock<[[usize; 2]; 2]> = 
    SpinLock::new([[0; 2]; 2]);

static START: AtomicBool = AtomicBool::new(false);
static SECONDATY_HOLDING: AtomicBool = AtomicBool::new(false);
static RELEASE_PAGES: AtomicBool = AtomicBool::new(false);
static SECONDARY_DONE: AtomicBool = AtomicBool::new(false);


fn allocate_and_record(role: usize) -> [usize; 2] {
    let kernel_pa = pmem::alloc_kernel();
    let data_pa = pmem::alloc_data();
    let pages = [kernel_pa, data_pa];

    {
        let mut held = HELD.lock();
        held[role] = pages;
    }

    pages
}

/// # Safety
/// page 必须指向一整页有效、可读且已初始化的内存；
/// 检查期间不能有其他执行者修改该页
fn assert_page_filled(page: usize, expected: u8) {
    for offset in 0..PAGE_SIZE {
        let actual = unsafe {
            (page as *const u8).add(offset).read()
        };
        assert_eq!(actual, expected);
    }
}

/// # Safety
/// pages[0]、pages[1] 分别是当前独占的内核池、数据池页面；
/// 两页互不重叠、尚未归还，且没有其他访问者
unsafe fn check_and_return(pages: [usize; 2], pattern: u8) {
    for pa in pages {
        unsafe {
            assert_page_filled(pa, 0);

            core::ptr::write_bytes(
                pa as *mut u8,
                pattern,
                PAGE_SIZE,
            );

            assert_page_filled(pa, pattern);
        }
    }

    unsafe {
        pmem::free_kernel(pages[0]);
        pmem::free_data(pages[1]);
    }
}

pub fn run_primary() {
    let before = free_counts();

    START.store(true, Ordering::Release);

    let pages = allocate_and_record(cpu::hart_id());

    while !SECONDATY_HOLDING.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    let held = {
        let guard = HELD.lock();
        *guard
    };

    let addresses = [
        held[0][0],
        held[0][1],
        held[1][0],
        held[1][1],
    ];
    let _ = console::print(format_args!(
        concat!(
            "hart0: kernel_page:[{}], data_page:[{}]\r\n",
            "hart1: kernel_page:[{}], data_page:[{}]\r\n"
        ),
        addresses[0],
        addresses[1],
        addresses[2],
        addresses[3],
    ));
    for i in 0..4 {
        for j in i+1..4 {
            assert_ne!(addresses[i], addresses[j]);
        }
    }
    assert_eq!(free_counts(), (before.0 -2, before.1 - 2));

    RELEASE_PAGES.store(true, Ordering::Release);

    unsafe {
        check_and_return(pages, 0xAA);
    }

    while !SECONDARY_DONE.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    assert_eq!(free_counts(), before);
    pmem::check_all_free();

    let _ = console::print(format_args!(
        "pmem:: two-hart allocation checks passed\r\n",
    ));
}

pub fn run_secondary() {
    while !START.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    let pages = allocate_and_record(cpu::hart_id());

    SECONDATY_HOLDING.store(true, Ordering::Release);

    while !RELEASE_PAGES.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    unsafe {
        check_and_return(pages, 0xBB);
    }

    SECONDARY_DONE.store(true, Ordering::Release);
}