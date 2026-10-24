core::arch::global_asm!(include_str!("trampoline.S"));

unsafe extern "C" {
    static __trampoline_start: u8;

    #[cfg(feature = "user-vm-test")]
    fn _trampoline_probe() -> usize;

    #[cfg(feature = "user-vm-test")]
    fn _user_return(frame: usize, satp: usize) -> !;

    #[cfg(feature = "user-vm-test")]
    fn _user_trap() -> !;
}

pub fn pa() -> usize {
    &raw const __trampoline_start as usize
}


#[cfg(feature = "user-vm-test")]
fn high_alias(linked: usize) -> usize {
    use crate::mem::{PAGE_SIZE, layout::TRAMPOLINE};

    let offset = linked
        .checked_sub(pa())
        .expect("entry is before trampoline");

    assert!(offset < PAGE_SIZE);
    TRAMPOLINE + offset
}

#[cfg(feature = "user-vm-test")]
pub(crate) fn user_return_entry() -> usize {
    high_alias(_user_return as *const () as usize)
}

#[cfg(feature = "user-vm-test")]
pub(crate) fn user_trap_entry() -> usize {
    high_alias(_user_trap as *const () as usize)
}

#[cfg(feature = "user-vm-test")]
/// # Safety
/// 当前使用包含 trampoline 高地址 RX 映射的内核页表，且调用期间映射持续有效
pub unsafe fn check_alias() {
    use crate::console;

    let linked  = _trampoline_probe as *const () as usize;

    let entry = high_alias(linked);

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