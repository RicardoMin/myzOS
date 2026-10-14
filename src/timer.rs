use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{cpu, irq, sbi, spinlock::SpinLock};


pub const TIMER_INTERVAL: usize = 1_000_000;

static BOOT_HART: AtomicUsize = AtomicUsize::new(usize::MAX);
static TICKS: SpinLock<usize> = SpinLock::new(0);

static INTERRUPTS: [AtomicUsize; cpu::MAX_HARTS] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];


pub fn init(boot_hart: usize) {
    assert!(boot_hart < cpu::MAX_HARTS);
    assert_eq!(boot_hart, cpu::hart_id());

    let probe = sbi::probe_timer();
    assert_eq!(probe.error, 0, "timer: SBI TIME probe failed");
    assert!(probe.value != 0, "timer: SBI TIME is unavailable");

    BOOT_HART.store(boot_hart, Ordering::Release);
}

fn read_time() -> usize {
    let value: usize;

    unsafe {
        asm!(
            "rdtime {value}",
            value = out(reg) value,
            options(nostack),
        );
    }

    value
}

fn schedule_next() {
    let deadline = read_time().wrapping_add(TIMER_INTERVAL);

    let result = sbi::set_timer(deadline);

    assert_eq!(result.error, 0, "timer: SBI set_timer failed");
}

pub fn init_hart() {
    let mut sstatus: usize;

    unsafe {
        asm!(
            "csrr {status}, sstatus",
            status = out(reg) sstatus,
            options(nostack),
        );
    }

    assert_eq!(sstatus & (1 << 1), 0);
    assert_eq!(irq::depth(), 0);
    assert!(BOOT_HART.load(Ordering::Acquire) < cpu::MAX_HARTS);

    schedule_next();

    unsafe {
        asm!(
            "csrs sie, {mask}",
            mask = in(reg) (1usize << 5),
            options(nostack),
        );
    }
}

pub fn on_interrupt() {
    schedule_next();

    let hart = cpu::hart_id();
    INTERRUPTS[hart].fetch_add(1, Ordering::Relaxed);

    if hart == BOOT_HART.load(Ordering::Relaxed) {
        let mut guard = TICKS.lock();
        *guard += 1;
    }
}

pub fn ticks() -> usize {
    *TICKS.lock()
}

pub fn interrupt_count(hart: usize) -> usize {
    INTERRUPTS[hart].load(Ordering::Relaxed)
}