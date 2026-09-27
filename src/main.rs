#![no_std]
#![no_main]


mod uart;
mod sbi;
mod spinlock;
mod irq;
mod cpu;


use core::panic::PanicInfo;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};


core::arch::global_asm!(include_str!("entry.S"));

unsafe extern "C" {
    pub fn _secondary_entry(hart_id: usize, opaque: usize) -> !;
}


static SECONDARY_READY: AtomicBool = AtomicBool::new(false);
static  SECONDARY_HART: AtomicUsize = AtomicUsize::new(usize::MAX);

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


#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(
    _hart_id: usize,
    _dtb_pa: usize
) -> ! {
    assert_eq!(cpu::hart_id(), _hart_id);

    let mut writer = uart::Writer::new();

    let _ = write!(
        &mut writer, 
        "myzOS: hart={} dtb={:#x}\r\n", 
        _hart_id,
         _dtb_pa,
    );

    let probe = sbi::probe_hsm();

    let _ = write!(
        &mut writer,
        "HSM probe: error={} value={}\r\n",
        probe.error,
        probe.value,
    );

    if probe.error == 0 && probe.value != 0 {
        for hart_id in 0..2 {
            let result = sbi::hart_get_status(hart_id);

            if result.error == 0 {
                let _ = write!(
                    &mut writer,
                    "hart {}: state={}\r\n",
                    hart_id,
                    result.value,
                );
            } else {
                let _ = write!(
                    &mut writer,
                    "hart {}: query failed, error={}\r\n",
                    hart_id,
                    result.error,
                );
            }
        }

        let target_hart = match _hart_id {
            0 => 1,
            1 => 0,
            _ => loop {},
        };
        let status = sbi::hart_get_status(target_hart);

        if status.error != 0 || status.value != 1 {
            let _ = write!(
                &mut writer,
                "hart {}: query failed or not stopped, error={}, state={}\r\n",
                target_hart,
                status.error,
                status.value,
            );
            
            loop {}
        }

        let start_addr = _secondary_entry as *const () as usize;
        let _ = write!(
            &mut writer,
            "boot hart {}: starting hart {}\r\n",
            _hart_id,
            target_hart,
        );

        guard_return_probe(true);

        {
            let _guard = COUNTER_LOCK.lock();
        }

        let _ = write!(
            &mut writer,
            "guard: reacquired after return\r\n",
        );

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

        let _ = write!(
            &mut writer,
            "nested irq: depths={:?} resotred={}\r\n",
            depths,
            irq::enabled(),
        );

        // ============================ 启动副 hart ==================================

        let result = unsafe {
            sbi::hart_start(target_hart, start_addr, 0x1234)
        };

        if result.error != 0 {
            let _ = write!(
                &mut writer,
                "start hart {} failed, error={}\r\n",
                target_hart,
                result.error,
            );

            loop {}
        }

        while !SECONDARY_READY.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }

        let reported_hart = SECONDARY_HART.load(Ordering::Relaxed);

        let _ = write!(
            &mut writer,
            "boot hart {}: observed secondary {} ready\r\n",
            _hart_id,
            reported_hart,
        );

        START_COUNTING.store(true, Ordering::Release);

        for _ in 0..ITERATIONS {
            increment_counter();
        }

        while !SECONDARY_DONE.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }

        assert_eq!(irq::depth(), 0);
        assert!(!irq::enabled());

        let actual = COUNTER.load(Ordering::Relaxed);
        let _ = write!(
            &mut writer,
            "counter: expected={} actual={}\r\n",
            2 * ITERATIONS,
            actual,
        );
    }

    loop {}
}


#[unsafe(no_mangle)]
pub extern "C" fn secondary_main(
    hart_id: usize,
    _opaque: usize,
) -> ! {
    assert_eq!(cpu::hart_id(), hart_id);

    SECONDARY_HART.store(hart_id, Ordering::Relaxed);
    SECONDARY_READY.store(true, Ordering::Release);

    while !START_COUNTING.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }

    for _ in 0..ITERATIONS {
        increment_counter();
    }

    assert_eq!(irq::depth(), 0);
    assert!(!irq::enabled());

    SECONDARY_DONE.store(true, Ordering::Release);

    loop {}
}
