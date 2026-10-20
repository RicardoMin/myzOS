use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    console,
    context::{Context, arch_switch},
    cpu, irq,
};

const STACK_SIZE: usize = 16 * 1024;

#[repr(C, align(16))]
struct TestStack {
    _bytes: [u8; STACK_SIZE],
}

static mut STACK: TestStack = TestStack {
    _bytes: [0; STACK_SIZE],
};

static mut BOOT: Context = Context::ZERO;
static mut DEMO: Context = Context::ZERO;
static ROUNDS: AtomicUsize = AtomicUsize::new(0);


extern "C" fn entry() -> ! {
    let bottom = (&raw mut STACK) as *mut u8 as usize;

    let sp: usize;
    unsafe {
        asm!(
            "mv {}, sp",
            out(reg) sp,
            options(nostack),
        );
    }

    assert!((bottom..bottom + STACK_SIZE).contains(&sp));
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
/// 仅启动 hart 调用一次；关中断且不持锁，独占本模块上下文和栈
pub unsafe fn run() {
    assert!(!irq::enabled());
    assert_eq!(irq::depth(), 0);

    let hart = cpu::hart_id();
    let top = (&raw mut STACK) as *mut u8 as usize + STACK_SIZE;

    unsafe {
        (&raw mut DEMO).write(Context {
            ra: entry as *const () as usize,
            sp: top,
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

    let _ = console::print(format_args!(
        "context-test: hart={} rounds=2 stack restored\r\n",
        hart,
    ));
}