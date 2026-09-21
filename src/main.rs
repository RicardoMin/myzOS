#![no_std]
#![no_main]

mod uart;

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

    loop {}
}
