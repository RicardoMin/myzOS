use core::arch::asm;

pub const MAX_HARTS: usize = 2;

pub fn hart_id() -> usize {
    let id: usize;

    unsafe {
        asm!(
            "mv {id}, tp",
            id = out(reg) id,
            options(nostack),
        );
    }

    id
}