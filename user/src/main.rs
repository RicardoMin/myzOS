#![no_std]
#![no_main]

mod syscall;

use core::{panic::PanicInfo, mem::MaybeUninit};
use core::arch::asm;
use myzos_uapi::{
    SYS_HELLO,
    SYS_TEST_COPYIN,
    SYS_TEST_COPYOUT,
    SYS_TEST_COPYINSTR,
    E_BADARG,
};
use syscall::{
    brk,
    syscall6,
    mmap,
};


#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}


unsafe fn check_brk() -> bool {
    let initial_top = unsafe {brk(0)};
    if initial_top != 0x2000 {
        return false;
    }

    let mut new_top = unsafe {brk(0x4000)};
    if new_top != 0x4000 {
        return false;
    }

    let first = 0x2000 as *mut u8;
    let second = 0x3000 as *mut u8;
    unsafe {
        let initial_first = first.read_volatile();
        first.write_volatile(0x11);
        let initial_second = second.read_volatile();
        second.write_volatile(0x22);

        if initial_first != 0 || initial_second != 0 {
            return false;
        }
    }

    new_top = unsafe {brk(0x4000)};
    if new_top != 0x4000 {
        return false;
    }

    let wrong_top = unsafe {brk(0x4001)};
    if wrong_top != E_BADARG {
        return false;
    }

    new_top = unsafe{brk(0x3000)};
    if new_top != 0x3000 {
        return false;
    }

    let denied = unsafe {
        syscall6(
            SYS_TEST_COPYIN,
            [0x3000, 1, 0, 0, 0, 0]
        )
    };
    if denied != E_BADARG {
        return false;
    }

    new_top = unsafe{brk(0x4000)};
    if new_top != 0x4000 {
        return false;
    }
    unsafe {
        let after_first = first.read_volatile();
        let after_second = second.read_volatile();
        if after_first != 0x11 || after_second != 0 {
            return false;
        }
    }

    new_top = unsafe{brk(0x2000)};
    if new_top != initial_top {
        return false;
    }

    true
}

#[inline(never)]
fn check_stack() -> bool {
    let mut storage = MaybeUninit::<[u8; 4 * 4096]>::uninit();
    let p = storage.as_mut_ptr().cast::<u8>();

    unsafe {
        for (i, &byte) in b"stack high\0".iter().enumerate() {
            p.add(3 * 4096 + i).write_volatile(byte);
        }

        let high = syscall6(
            SYS_TEST_COPYINSTR,
            [p.add(3 * 4096) as usize, 0, 0, 0, 0, 0],
        );

        for (i, &byte) in b"stack low\0".iter().enumerate() {
            p.add(i).write_volatile(byte);
        }

        let low = syscall6(
            SYS_TEST_COPYINSTR,
            [p as usize, 0, 0, 0, 0, 0],
        );

        high == 0 && low == 0
    }
}

fn check_mmap() -> bool {
    const PAGE: usize = 4096;
    const MMAP_BEGIN: usize =
        (1usize << 38) - (4096 + 2 + 16384) * PAGE;

    let first = mmap(MMAP_BEGIN, PAGE) as usize;
    let second = mmap(0, PAGE) as usize;

    if first != MMAP_BEGIN || second != MMAP_BEGIN + PAGE {
        return false;
    }

    unsafe {
        if (first as *const u8).read_volatile() != 0
        || ((first + PAGE - 1) as *const u8).read_volatile() != 0
        {
            return false;
        }
        if (second as *const u8).read_volatile() != 0
        || ((second + PAGE - 1) as *const u8).read_volatile() != 0
        {
            return false;
        }

        (first as *mut u8).write_volatile(0x11);
        (second as *mut u8).write_volatile(0x22);
        if (first as *const u8).read_volatile() != 0x11 {
            return false;
        }
        if (second as *const u8).read_volatile() != 0x22 {
            return false;
        }
    }

    true
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

    let heap_ok = unsafe {check_brk()};

    let stack_ok = check_stack();

    let mmap_ok = check_mmap();

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
            in("a7") (copyinstr_ok && heap_ok && stack_ok && mmap_ok) as usize,
            options(nostack),
        );
    }

    loop {
        core::hint::spin_loop();
    }
}