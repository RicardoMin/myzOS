use core::arch::asm;

pub struct SbiRet {
    pub error: isize,
    pub value: usize,
}

const EID_BASE: usize = 0x10;
const EID_HSM: usize = 0x48534D;

unsafe extern "C" {

    pub fn _secondary_entry(hart_id: usize, opaque: usize) -> !;
}


fn sbi_call1(eid: usize, fid: usize, arg0: usize) -> SbiRet {
    let error: isize;
    let value: usize;

    unsafe {
        asm!(
            "ecall",
            inlateout("a0") arg0 => error,
            lateout("a1") value,
            in("a6") fid,
            in("a7") eid,
        );
    }

    SbiRet { error, value }
}

pub fn probe_hsm() -> SbiRet {
    sbi_call1(EID_BASE, 3, EID_HSM)
}

pub fn hart_get_status(hart_id: usize) -> SbiRet {
    sbi_call1(EID_HSM, 2, hart_id)
}


/// # Safety
/// start_addr 必须指向有效的物理入口
/// 目标入口必须建立独立栈，调用者必须保证共享数据和设备的访问安排正确
pub unsafe fn hart_start(
    hart_id: usize,
    start_addr: usize,
    opaque: usize,
) -> SbiRet {
    let error: isize;
    let value: usize;

    unsafe {
        asm!(
            "ecall",
            inlateout("a0") hart_id => error,
            inlateout("a1") start_addr => value,
            in("a2") opaque,
            in("a6") 0usize,
            in("a7") EID_HSM,
        );
    }

    SbiRet { error, value }
}