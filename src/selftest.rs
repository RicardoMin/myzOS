use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::{console, cpu, irq, spinlock};


const ITERATIONS: usize = 100_000;

static COUNTER: AtomicUsize = AtomicUsize::new(0);
static COUNTER_LOCK: spinlock::RawSpinLock =
    spinlock::RawSpinLock::new();

static START_COUNTING: AtomicBool = AtomicBool::new(false);
static SECONDARY_DONE: AtomicBool = AtomicBool::new(false);


fn increment_counter() {
    let _guard = COUNTER_LOCK.lock();

    let value = COUNTER.load(Ordering::Relaxed);
    COUNTER.store(value + 1, Ordering::Relaxed);
}


fn guard_return_probe(leave_early: bool) {
    let _guard = COUNTER_LOCK.lock();

    if leave_early {
        return;
    }
}

fn console_probe(hart_id: usize) {
    for seq in 0..100 {
        let _ = console::print(format_args!(
            "[hart={} seq={:03}] ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789 abcdefghijklmnopqrstuvwxyz end\r\n",
            hart_id,
            seq,
        ));
    }
}


pub fn boot_checks() {
    guard_return_probe(true);

    {
        let _guard = COUNTER_LOCK.lock();
    }

    let _ = console::print(format_args!(
        "guard: reacquired after return\r\n",
    ));

    let before = irq::enabled();

    let lock_a = spinlock::RawSpinLock::new();
    let lock_b = spinlock::RawSpinLock::new();

    let d0 = irq::depth();

    let guard_a = lock_a.lock();
    let d1 = irq::depth();
    assert!(!irq::enabled());

    let guard_b = lock_b.lock();
    let d2 = irq::depth();
    assert!(!irq::enabled());

    core::mem::drop(guard_a);
    let d3 = irq::depth();
    assert!(!irq::enabled());

    core::mem::drop(guard_b);
    let d4 = irq::depth();

    let depths = [d0, d1, d2, d3, d4];
    assert_eq!(depths, [0, 1, 2, 1, 0]);
    assert_eq!(irq::enabled(), before);

    let _ = console::print(format_args!(
        "nested irq: depths={:?} restored={}\r\n",
        depths,
        irq::enabled(),
    ));
}


pub fn run_primary() {
    START_COUNTING.store(true, Ordering::Release);

    console_probe(cpu::hart_id());

    for _ in 0..ITERATIONS {
        increment_counter();
    }

    while !SECONDARY_DONE.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    assert_eq!(irq::depth(), 0);
    assert!(!irq::enabled());

    let actual = COUNTER.load(Ordering::Relaxed);
    let _ = console::print(format_args!(
        "counter: expected={} actual={}\r\n",
        2 * ITERATIONS,
        actual,
    ));
}


pub fn run_secondary(hart_id: usize) {
    while !START_COUNTING.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    console_probe(hart_id);

    for _ in 0..ITERATIONS {
        increment_counter();
    }

    assert_eq!(irq::depth(), 0);
    assert!(!irq::enabled());

    SECONDARY_DONE.store(true, Ordering::Release);
}