use core::arch::asm;


/// # Safety
/// 调用者须遵守所选系统调用的内存、别名和生命周期约束
pub unsafe fn syscall6(number: usize, args: [usize; 6]) -> isize {
    let result: usize;

    unsafe {
        asm!(
            "ecall",
            inlateout("a0") args[0] => result,
            in("a1") args[1],
            in("a2") args[2],
            in("a3") args[3],
            in("a4") args[4],
            in("a5") args[5],
            in("a7") number,
            options(nostack),
        );
    }

    result as isize
}


/// # Safety
/// 收缩区域内不存在仍需使用的对象或引用；成功后不得访问已释放区域
pub unsafe fn brk(new_top: usize) -> isize {
    unsafe {
        syscall6(
            myzos_uapi::SYS_BRK,
            [new_top, 0, 0, 0, 0, 0],
        )
    }
}

pub fn mmap(begin: usize, len: usize) -> isize {
    // SAFETY: 该调用仅新增映射，不解除或覆盖现有区域
    unsafe {
        syscall6(
            myzos_uapi::SYS_MMAP,
            [begin, len, 0, 0, 0, 0],
        )
    }
}

/// # Safety
/// 被解除的区域中没有仍需使用的对象或引用，成功后调用者不得再访问已释放页面
pub unsafe fn munmap(begin: usize, len: usize) -> isize {
    unsafe {
        syscall6(
            myzos_uapi::SYS_MUNMAP,
            [begin, len, 0, 0, 0, 0],
        )
    }
}