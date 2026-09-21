use core::arch::asm;

pub struct SbiRet {
    pub error: isize,
    pub value: usize,
}

const EID_BASE: usize = 0x10;
const EID_HSM: usize = 0x48534D;

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