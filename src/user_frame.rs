use crate::trap::TrapFrame;

#[repr(C)]
pub struct UserFrame {
    pub regs: TrapFrame,
    pub kernel_satp: usize,
    pub kernel_sp: usize,
    pub kernel_entry: usize,
    pub kernel_hart: usize,
    pub kernel_gp: usize,
}


const _: () = {
    assert!(core::mem::size_of::<UserFrame>() == 312);
    assert!(core::mem::offset_of!(UserFrame, regs) == 0);
    assert!(core::mem::offset_of!(UserFrame, kernel_satp) == 272);
    assert!(core::mem::offset_of!(UserFrame, kernel_sp) == 280);
    assert!(core::mem::offset_of!(UserFrame, kernel_entry) == 288);
    assert!(core::mem::offset_of!(UserFrame, kernel_hart) == 296);
    assert!(core::mem::offset_of!(UserFrame, kernel_gp) == 304);
};


impl UserFrame {
    pub fn new(entry: usize, stack_top: usize) -> Self {
        let mut trapframe = TrapFrame {
            x: [0; 32],
            sepc: entry,
            sstatus: 0,
        };

        trapframe.x[2] = stack_top;

        Self {
            regs: trapframe,
            kernel_satp: 0,
            kernel_sp: 0,
            kernel_entry: 0,
            kernel_hart: 0,
            kernel_gp: 0,
        }
    }
}