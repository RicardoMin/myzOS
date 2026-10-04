#![no_std]
#![no_main]


#[cfg(feature = "lab1-tests")]
mod selftest;
mod uart;
mod sbi;
mod spinlock;
mod irq;
mod cpu;
mod console;
mod mem;


use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::mem::PAGE_SIZE;
use crate::mem::RAM_START;
use crate::mem::RAM_END;
use crate::mem::pmem::{
    alloc_kernel,
    free_kernel,
    alloc_data,
    free_data,
    free_counts,
};

core::arch::global_asm!(include_str!("entry.S"));

unsafe extern "C" {
    pub fn _secondary_entry(hart_id: usize, opaque: usize) -> !;
}


static SECONDARY_READY: AtomicBool = AtomicBool::new(false);
static  SECONDARY_HART: AtomicUsize = AtomicUsize::new(usize::MAX);


/// # Safety
/// pa 指向当前持有的、可读且已初始化的一整页；
/// 检查期间没有其他执行者修改该页
unsafe fn assert_page_filled(pa: usize, expected: u8) {
    for offset in 0..PAGE_SIZE {
        let actual = unsafe {
            (pa as *const u8).add(offset).read()
        };
        assert_eq!(actual, expected);
    }
}

fn check_page_pool(pool: &mut mem::pmem::PagePool) {
    let before = pool.free_pages();
    assert!(before > 1);

    let page_a = pool.alloc();
    let page_b = pool.alloc();
    assert_ne!(page_a, page_b);
    assert_eq!(pool.free_pages(), before - 2);

    pool.check_free_chain();
    unsafe {
        assert_page_filled(page_a, 0);
        assert_page_filled(page_b, 0);

        core::ptr::write_bytes(page_a as *mut u8, 0xAA, PAGE_SIZE);
        core::ptr::write_bytes(page_b as *mut u8, 0x55, PAGE_SIZE);

        pool.free(page_a);
    }
    assert!(pool.free_pages() == before - 1);

    let page_c = pool.alloc();
    assert!(page_c == page_a);

    unsafe {
        assert_page_filled(page_c, 0);
        assert_page_filled(page_b, 0x55);

        pool.free(page_b);
        pool.free(page_c);
    }

    pool.check_initial_chain();
}


#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(
    _hart_id: usize,
    _dtb_pa: usize,
) -> ! {
    assert_eq!(cpu::hart_id(), _hart_id);

    let _ = console::print(format_args!(
        "myzOS: hart={} dtb={:#x}\r\n", 
        _hart_id,
         _dtb_pa,
    ));

    mem::report_layout(_dtb_pa);

    // SAFETY: 使用 OpenSBI 传入的启动参数
    // 当前尚未启用分页，读取期间不会修改设备树
    let dtb = unsafe {
        mem::dtb::inspect(_dtb_pa)
    };

    let reserved_start = mem::page_floor(dtb.start);
    let reserved_end = mem::page_ceil(dtb.end)
        .expect("DTB page range overflow");

    assert!(RAM_START <= reserved_start);
    assert!(reserved_end <= RAM_END);

    let kernel = mem::kernel_layout();

    assert!(
        reserved_end <= kernel.start
            || kernel.end <= reserved_start
    );

    let _ = console::print(format_args!(
        concat!(
            "dtb: bytes=[{:#x}, {:#x})\r\n",
            "dtb: pages=[{:#x}, {:#x})\r\n",
            "dtb: cover_pages={}\r\n",
        ),
        dtb.start,
        dtb.end,
        reserved_start,
        reserved_end,
        {reserved_end - reserved_start} / PAGE_SIZE,
    ));

    // =============== pool 范围 ==============================

    let pool_begin = kernel.end;
    let pool_end = reserved_start;

    assert!(pool_begin < pool_end);

    let split = pool_begin
        .checked_add(mem::pmem::KERNEL_PAGES * PAGE_SIZE)
        .expect("kernel pool boundary overflow");

    assert!(split < pool_end);

    let _ = console::print(format_args!(
        concat!(
            "kernel_pool: [{:#x}, {:#x})\r\n",
            "data_pool: [{:#x}, {:#x})\r\n",
        ),
        pool_begin,
        split,
        split,
        pool_end,
    ));

    // SAFETY: 当前固定 QEMU/OpenSBI 布局已经核对，
    // 这两个范围排除了内核、固件和 DTB，互不重叠；
    // 此时仅主 hart 初始化，其他执行者不访问这些页面
    let mut kernel_pool = unsafe {
        mem::pmem::PagePool::new(pool_begin, split)
    };

    let mut data_pool = unsafe {
        mem::pmem::PagePool::new(split, pool_end)
    };

    let _ = console::print(format_args!(
        "kernel_pool begins check\r\n",
    ));
    {
        kernel_pool.check_initial_chain();
        assert_eq!(kernel_pool.free_pages(), mem::pmem::KERNEL_PAGES);
        check_page_pool(&mut kernel_pool);
    }
    let _ = console::print(format_args!(
        "kernel pool: alloc/free/zero checks passed\r\n"
    ));

    let _ = console::print(format_args!(
        "data_pool begins check\r\n",
    ));
    {
        data_pool.check_initial_chain();
        assert_eq!(data_pool.free_pages(), (pool_end - split) / PAGE_SIZE);
        check_page_pool(&mut data_pool);
    }
    let _ = console::print(format_args!(
        "data pool: alloc/free/zero checks passed\r\n"
    ));

    mem::pmem::init(kernel_pool, data_pool);

    let before = free_counts();
    let kernel_pa = alloc_kernel();
    let data_pa = alloc_data();

    unsafe {
        free_kernel(kernel_pa);
        free_data(data_pa);
    }
    assert_eq!(free_counts(), before);
    let _ = console::print(format_args!(
        "global page pool is ready\r\n",
    ));

    let probe = sbi::probe_hsm();

    let _ = console::print(format_args!(
        "HSM probe: error={} value={}\r\n",
        probe.error,
        probe.value,
    ));

    if probe.error == 0 && probe.value != 0 {
        for hart_id in 0..2 {
            let result = sbi::hart_get_status(hart_id);

            if result.error == 0 {
                let _ = console::print(format_args!(
                    "hart {}: state={}\r\n",
                    hart_id,
                    result.value,
                ));
            } else {
                let _ = console::print(format_args!(
                    "hart {}: query failed, error={}\r\n",
                    hart_id,
                    result.error,
                ));
            }
        }

        let target_hart = match _hart_id {
            0 => 1,
            1 => 0,
            _ => loop {},
        };
        let status = sbi::hart_get_status(target_hart);

        if status.error != 0 || status.value != 1 {
            let _ = console::print(format_args!(
                "hart {}: query failed or not stopped, error={}, state={}\r\n",
                target_hart,
                status.error,
                status.value,
            ));
            
            loop {}
        }

        let start_addr = _secondary_entry as *const () as usize;
        let _ = console::print(format_args!(
            "boot hart {}: starting hart {}\r\n",
            _hart_id,
            target_hart,
        ));

        #[cfg(feature = "lab1-tests")]
        selftest::boot_checks();

        // ============================ 启动副 hart ==================================

        let result = unsafe {
            sbi::hart_start(target_hart, start_addr, 0x1234)
        };

        if result.error != 0 {
            let _ = console::print(format_args!(
                "start hart {} failed, error={}\r\n",
                target_hart,
                result.error,
            ));

            loop {}
        }

        while !SECONDARY_READY.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }

        let reported_hart = SECONDARY_HART.load(Ordering::Relaxed);

        let _ = console::print(format_args!(
            "boot hart {}: observed secondary {} ready\r\n",
            _hart_id,
            reported_hart,
        ));

        #[cfg(feature = "lab1-tests")]
        selftest::run_primary();

        #[cfg(feature = "lab2-tests")]
        mem::selftest::run_primary();

        let _ = console::print(format_args!(
            "myzOS: boot complete, {} harts online\r\n",
            cpu::MAX_HARTS,
        ));
    }

    loop {}
}


#[unsafe(no_mangle)]
pub extern "C" fn secondary_main(
    hart_id: usize,
    opaque: usize,
) -> ! {
    assert_eq!(cpu::hart_id(), hart_id);

    let _ = console::print(format_args!(
        "secondary: hart={} opaque={:#x}\r\n",
        hart_id,
        opaque,
    ));

    SECONDARY_HART.store(hart_id, Ordering::Relaxed);
    SECONDARY_READY.store(true, Ordering::Release);

    #[cfg(feature = "lab1-tests")]
    selftest::run_secondary(hart_id);

    #[cfg(feature = "lab2-tests")]
    mem::selftest::run_secondary();

    loop {}
}