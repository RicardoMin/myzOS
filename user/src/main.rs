#![no_std]
#![no_main]

mod syscall;

use core::panic::PanicInfo;
use core::arch::asm;
use myzos_uapi::{
    SYS_HELLO,
    SYS_TEST_COPYIN,
    SYS_TEST_COPYOUT,
    SYS_TEST_COPYINSTR,
    E_BADARG,
};
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

    let mut output = [0i32; 5];

    let written = unsafe {
        syscall6(
            SYS_TEST_COPYOUT,
            [output.as_mut_ptr() as usize, 0, 0, 0, 0, 0],
        )
    };

    let read_back = unsafe {
        syscall6(
            SYS_TEST_COPYIN,
            [output.as_ptr() as usize, output.len(), 0, 0, 0, 0],
        )
    };

    let copyout_ok =
        written == 0 && output == [1, 2, 3, 4, 5] && read_back == 0;

    let message = b"hello from user\0ignored";
    let unterminated = [b'x'; 64];

    let string_valid = unsafe {
        syscall6(
            SYS_TEST_COPYINSTR,
            [message.as_ptr() as usize, 0, 0, 0, 0, 0],
        )
    };

    let string_unterminated = unsafe {
        syscall6(
            SYS_TEST_COPYINSTR,
            [unterminated.as_ptr() as usize, 0, 0, 0, 0, 0],
        )
    };

    let string_denied = unsafe {
        syscall6(
            SYS_TEST_COPYINSTR,
            [(1usize << 38) - 2 * 4096, 0, 0, 0, 0, 0],
        )
    };

    let copyinstr_ok = string_valid == 0
        && string_unterminated == E_BADARG
        && string_denied == E_BADARG;

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
            in("a6") copyout_ok as usize,
            in("a7") copyinstr_ok as usize,
            options(nostack),
        );
    }

    loop {
        core::hint::spin_loop();
    }
}