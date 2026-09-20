#![no_std]
#![no_main]

core::arch::global_asm!(include_str!("entry.S"));

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(
    _hart_id: usize,
    _dtb_pa: usize,
) -> ! {
    loop {}
}