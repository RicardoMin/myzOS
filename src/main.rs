#![no_std]
#![no_main]

mod uart;
mod sbi;
mod spinlock;
mod irq;
mod cpu;
mod console;
mod mem;
mod trap;
mod timer;
mod plic;
mod input;
mod line;
mod shell;
mod trampoline;
#[cfg(feature = "lab1-tests")]
mod selftest;
#[cfg(any(
    feature = "context-test",
    feature = "user-vm-test",
))]
mod context;
#[cfg(feature = "context-test")]
mod context_test;
#[cfg(feature = "user-vm-test")]
mod user_frame;
#[cfg(feature = "user-vm-test")]
mod proc;


use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::fmt::Write;

use crate::mem::{PAGE_SIZE, RAM_END, RAM_START};

core::arch::global_asm!(include_str!("entry.S"));

unsafe extern "C" {
    pub fn _secondary_entry(hart_id: usize, opaque: usize) -> !;
}


static SECONDARY_READY: AtomicBool = AtomicBool::new(false);
static  SECONDARY_HART: AtomicUsize = AtomicUsize::new(usize::MAX);

static PANICKING: AtomicBool = AtomicBool::new(false);

static KERNEL_ROOT_PA: AtomicUsize = AtomicUsize::new(0);
static SECONDARY_PAGING_READY: AtomicBool = AtomicBool::new(false);


#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    let _ = irq::save_and_disable();

    let first = !PANICKING.swap(true, Ordering::Relaxed);

    if first {
        let mut writer = uart::Writer::new();

        let _ = write!(
            &mut writer,
            "\r\nPANIC hart={}: {}\r\n",
            cpu::hart_id(),
            info,
        );
    }

    loop {
        core::hint::spin_loop();
    }
}


#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(
    _hart_id: usize,
    _dtb_pa: usize,
) -> ! {
    assert_eq!(cpu::hart_id(), _hart_id);

    // stvec 属于当前 hart；两个启动入口必须各自安装异常入口
    trap::init_hart();
    timer::init(_hart_id);

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

    #[cfg(feature = "lab2-tests")]
    mem::selftest::check_vm_encoding();

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
    let kernel_pool = unsafe {
        mem::pmem::PagePool::new(pool_begin, split)
    };

    let data_pool = unsafe {
        mem::pmem::PagePool::new(split, pool_end)
    };

    #[cfg(feature = "lab2-tests")]
    let (kernel_pool, data_pool) = {
        let (mut kernel_pool, mut data_pool) = (kernel_pool, data_pool);
        mem::selftest::check_exhaustion(&mut kernel_pool);
        mem::selftest::check_exhaustion(&mut data_pool);
        (kernel_pool, data_pool)
    };

    mem::pmem::init(kernel_pool, data_pool);

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

        if status.error != 0 {
            let _ = console::print(format_args!(
                "query failed, error: {}\r\n",
                status.error,
            ));

            loop {}
        }

        if status.value != 1 {
            let _ = console::print(format_args!(
                "target hart is not stopped, state={}\r\n",
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

        let before = mem::pmem::free_counts();
        let root_pa = mem::kernel_vm::build();
        let after = mem::pmem::free_counts();

        let _ = console::print(format_args!(
            "kernel_vm: root={:#x}, kernel_pages_used={}, data_pages_used={}\r\n",
            root_pa,
            before.0 - after.0,
            before.1 - after.1,
        ));

        #[cfg(feature = "lab2-tests")]
        unsafe {
            // SAFETY: 页表刚由当前 hart 建好，尚未发布或交给硬件使用
            mem::selftest::check_kernel_mapping(root_pa);
        }

        // =================================== 启动分页 =========================

        // SAFETY: 页表由当前 hart 完整构建，恒等映射覆盖继续执行所需代码、
        // 栈、数据和设备；根表及下级表继续保留，发布后不再修改或释放
        unsafe { mem::kernel_vm::enable(root_pa) };

        let _ = console::print(format_args!(
            "paging: hart={} satp={:#x} Sv39 active\r\n",
            cpu::hart_id(),
            mem::kernel_vm::read_satp(),
        ));

        KERNEL_ROOT_PA.store(root_pa, Ordering::Release);

        while !SECONDARY_PAGING_READY.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        #[cfg(feature = "lab2-tests")]
        mem::selftest::check_page_tables();

        #[cfg(feature = "trap-test")]
        trap::check_return();

        timer::init_hart();

        plic::init_uart(_hart_id);
        uart::enable_rx_interrupt();
        unsafe {
            core::arch::asm!(
                "csrs sie, {mask}",
                mask = in(reg) (1usize << 9),
                options(nostack),
            );
        }

        let _ = console::print(format_args!(
            "timer: hart={} armed, interval={} time counts\r\n",
            cpu::hart_id(),
            timer::TIMER_INTERVAL,
        ));

        let _ = console::print(format_args!(
            "myzOS: boot complete, {} harts online\r\n",
            cpu::MAX_HARTS,
        ));

        let _ = console::print(format_args!(
            "myzOS console ready. Type help and press Enter.\r\n",
        ));

        #[cfg(feature = "context-test")]
        unsafe {
            // SAFETY: 仅启动 hart 调用一次，当前关中断不持锁
            context_test::run();
        }

        #[cfg(feature = "user-vm-test")]
        proc::check();

        // SAFETY: 本 hart 的异常入口和已使能来源的处理逻辑已就绪，当前不持锁
        unsafe { irq::enable() };

        let mut editor = line::LineEditor::new();

        loop {
            for _ in 0..32 {
                let Some(event) = input::next_event() else {
                    break;
                };

                match event {
                    input::InputEvent::Byte(byte) => {
                        if let Some(text) = editor.feed(byte) {
                            shell::execute(text);
                        }
                    }
                    input::InputEvent::Lost(count) => {
                        editor.on_input_lost(count);
                    }
                }
            }

            irq::push_off();

            if input::is_empty() {
                unsafe {
                    core::arch::asm!("wfi", options(nostack));
                }
            }

            // SAFETY: 同一 hart 上配对此处 push_off，期间保持关中断
            unsafe { irq::pop_off() };
        }
    }

    loop {}
}


#[unsafe(no_mangle)]
pub extern "C" fn secondary_main(
    hart_id: usize,
    opaque: usize,
) -> ! {
    assert_eq!(cpu::hart_id(), hart_id);

    trap::init_hart();

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

    let root_pa = loop {
        let root = KERNEL_ROOT_PA.load(Ordering::Acquire);

        if root != 0 {
            break root;
        }

        core::hint::spin_loop();
    };

    // SAFETY: Acquire 已接收到主 hart 发布的完整恒等页表，映射覆盖本 hart
    // 继续执行所需的代码、栈、数据和设备；页表持续保留，发布后不再修改或释放。
    unsafe { mem::kernel_vm::enable(root_pa) };

    let _ = console::print(format_args!(
        "paging: hart={} satp={:#x} Sv39 active\r\n",
        cpu::hart_id(),
        mem::kernel_vm::read_satp(),
    ));

    #[cfg(feature = "trap-test")]
    trap::check_return();

    timer::init_hart();

    let _ = console::print(format_args!(
        "timer: hart={} armed, interval={} time counts\r\n",
        cpu::hart_id(),
        timer::TIMER_INTERVAL,
    ));

    SECONDARY_PAGING_READY.store(true, Ordering::Release);

    unsafe { irq::enable() };

    loop {
        unsafe {
            core::arch::asm!("wfi", options(nostack));
        }
    }
}