#![no_std]
#![no_main]

mod uart;
mod sbi;

core::arch::global_asm!(include_str!("entry.S"));

use core::panic::PanicInfo;

use core::fmt::Write;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(
    _hart_id: usize,
    _dtb_pa: usize
) -> ! {
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
    }

    loop {}
}
