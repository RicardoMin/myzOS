#![no_std]
#![no_main]

mod syscall;

use core::panic::PanicInfo;
use core::arch::asm;
use myzos_uapi::{SYS_HELLO, SYS_TEST_COPYIN};
use syscall::syscall6;


#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    let first = unsafe { syscall6(SYS_HELLO, [0; 6]) };
    let second = unsafe { syscall6(SYS_HELLO, [0; 6]) };
    let unknown = unsafe { syscall6(usize::MAX, [0; 6]) };

    let values = [1i32, 2, 3, 4, 5, 6];

    let valid = unsafe {
        syscall6(
            SYS_TEST_COPYIN,
            [values.as_ptr() as usize, values.len(), 0, 0, 0, 0],
        )
    };

    let denied = unsafe {
        syscall6(
            SYS_TEST_COPYIN,
            [(1usize << 38) - 2* 4096, 1, 0, 0, 0, 0],
        )
    };

    let cross = unsafe {
        syscall6(
            SYS_TEST_COPYIN,
            [0x1ffe, 1, 0, 0, 0, 0],
        )
    };

    unsafe {
        asm!(
            ".option push",
            ".option norvc",
            "ebreak",
            ".option pop",
            in("a0") first,
            in("a1") second,
            in("a2") unknown,
            in("a3") valid,
            in("a4") denied,
            in("a5") cross,
            options(nostack),
        );
    }

    loop {
        core::hint::spin_loop();
    }
}