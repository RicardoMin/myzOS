core::arch::global_asm!(include_str!("trampoline.S"));

unsafe extern "C" {
    static __trampoline_start: u8;

    #[cfg(feature = "user-vm-test")]
    fn _trampoline_probe() -> usize;
}

pub fn pa() -> usize {
    &raw const __trampoline_start as usize
}


#[cfg(feature = "user-vm-test")]
/// # Safety
/// 当前使用包含 trampoline 高地址 RX 映射的内核页表，且调用期间映射持续有效
pub unsafe fn check_alias() {
    use crate::{
        console,
        mem::{PAGE_SIZE, layout::TRAMPOLINE},
    };

    let linked  = _trampoline_probe as *const () as usize;
    let offset = linked
        .checked_sub(pa())
        .expect("probe is before trampoline");

    assert!(offset < PAGE_SIZE);

    let entry = TRAMPOLINE + offset;

    let probe = unsafe {
        core::mem::transmute::<usize, unsafe extern "C" fn() -> usize>(
            entry,
        )
    };

    let observed = unsafe {probe() };
    assert_eq!(observed, entry);

    let _ = console::print(format_args!(
        "trampoline: high alias execution passed (kernel page table)\r\n",
    ));
}