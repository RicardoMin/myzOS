#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::arch::asm;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    unsafe {
        asm!(
            "ecall",
            inlateout("a0") 0x1234usize => _,
            in("a7") 0usize,
            options(nostack),
        );
    }

    loop {
        core::hint::spin_loop();
    }
}


#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}