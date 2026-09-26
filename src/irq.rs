use core::arch::asm;

const SSTATUS_SIE: usize = 1 << 1;

pub fn enabled() -> bool {
    let status: usize;

    unsafe {
        asm!(
            "csrr {status}, sstatus",
            status = out(reg) status,
            options(nostack),
        );
    }

    status & SSTATUS_SIE != 0
}


pub fn save_and_disable() -> bool {
    let old: usize;

    unsafe {
        asm!(
            "csrrc {old}, sstatus, {mask}",
            old = out(reg) old,
            mask = in(reg) SSTATUS_SIE,
            options(nostack),
        );
    }

    old & SSTATUS_SIE != 0
}


pub unsafe fn restore(was_enabled: bool) {
    if was_enabled {
        unsafe {
            asm!(
                "csrs sstatus, {mask}",
                mask = in(reg) SSTATUS_SIE,
                options(nostack),
            );
        }
    } else {
        unsafe {
            asm!(
                "csrc sstatus, {mask}",
                mask = in(reg) SSTATUS_SIE,
                options(nostack),
            )
        }
    }
}