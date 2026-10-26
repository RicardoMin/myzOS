#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::arch::asm;


fn syscall0(number: usize) -> isize {
    let result: usize;

    unsafe {
        asm!(
            "ecall",
            inlateout("a0") 0usize => result,
            in("a7") number,
            options(nostack),
        );
    }

    result as isize
}


#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    let first = syscall0(0);
    let second = syscall0(0);
    let unknown = syscall0(usize::MAX);

    unsafe {
        asm!(
            "ebreak",
            in("a0") first,
            in("a1") second,
            in("a2") unknown,
            options(nostack),
        );
    }

    loop {
        core::hint::spin_loop();
    }
}