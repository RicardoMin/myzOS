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


core::arch::global_asm!(include_str!("entry.S"));

unsafe extern "C" {
    pub fn _secondary_entry(hart_id: usize, opaque: usize) -> !;
}


static SECONDARY_READY: AtomicBool = AtomicBool::new(false);
static  SECONDARY_HART: AtomicUsize = AtomicUsize::new(usize::MAX);


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

    loop {}
}