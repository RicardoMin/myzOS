use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    console,
    context::{Context, arch_switch},
    cpu, irq,
};
use crate::mem::layout::{KSTACK_BOTTOM, KSTACK_TOP};

static mut BOOT: Context = Context::ZERO;
static mut DEMO: Context = Context::ZERO;
static ROUNDS: AtomicUsize = AtomicUsize::new(0);


extern "C" fn entry() -> ! {
    let sp: usize;
    unsafe {
        asm!(
            "mv {}, sp",
            out(reg) sp,
            options(nostack),
        );
    }

    assert!(KSTACK_BOTTOM <= sp && sp <= KSTACK_TOP);
    assert_eq!(sp % 16, 0);

    let mut round = 0usize;

    loop {
        round += 1;
        ROUNDS.store(round, Ordering::Relaxed);

        unsafe {
            arch_switch(&raw mut DEMO, &raw const BOOT);
        }
    }
}


/// # Safety
/// 仅启动 hart 调用一次；关中断且不持锁
/// 已启用包含 KSTACK 映射的内核页表，独占该栈和测试上下文
pub unsafe fn run() {
    assert!(!irq::enabled());
    assert_eq!(irq::depth(), 0);

    let hart = cpu::hart_id();

    unsafe {
        (&raw mut DEMO).write(Context {
            ra: entry as *const () as usize,
            sp: KSTACK_TOP,
            saved: [0; 12],
        });
    }

    for expected in 1..=2 {
        unsafe {
            arch_switch(&raw mut BOOT, &raw const DEMO);
        }

        assert_eq!(ROUNDS.load(Ordering::Relaxed), expected);
        assert_eq!(cpu::hart_id(), hart);
        assert!(!irq::enabled());
        assert_eq!(irq::depth(), 0);
    }

    unsafe {
        (&raw mut DEMO).write(Context::ZERO);
    }

    let _ = console::print(format_args!(
        "context-test: hart={} rounds=2 guarded kernel stack checked (boot stack restored)\r\n",
        hart,
    ));
}