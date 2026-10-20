core::arch::global_asm!(include_str!("switch.S"));

#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,
    pub saved: [usize; 12],
}


impl Context {
    pub const ZERO: Self = Self {
        ra: 0,
        sp: 0,
        saved: [0; 12],
    };
}


const _: () = {
    assert!(core::mem::size_of::<Context>() == 112);
    assert!(core::mem::offset_of!(Context, ra) == 0);
    assert!(core::mem::offset_of!(Context, sp) == 8);
    assert!(core::mem::offset_of!(Context, saved) == 16);
};


unsafe extern "C" {
    /// # Safety
    /// 上下文有效、独占且互不重叠；目标入口和栈有效
    /// 同一 hart 上关中断切换、不持锁，相关存储持续存活
    pub fn arch_switch(old: *mut Context, next: *const Context);
}