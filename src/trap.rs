use core::arch::asm;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{cpu, input, irq, plic, uart};

core::arch::global_asm!(include_str!("trap.S"));

unsafe extern "C" {
    fn _trap_entry();

    #[cfg(feature = "trap-test")]
    fn _trap_test() -> usize;

    #[cfg(feature = "trap-test")]
    fn _trap_test_breakpoint();
}

static TRAP_REPORTED: AtomicBool = AtomicBool::new(false);


#[cfg(feature = "trap-test")]
pub fn check_return() {
    // SAFETY: 当前 hart 已安装异常入口，并使用有效内核栈；
    // 指定测试断点会被处理函数识别，恢复现场后返回
    let value = unsafe { _trap_test() };

    assert_eq!(value, 0x1234);

    let _ = crate::console::print(format_args!(
        "trap-test: hart={} breakpoint returned, t0 restored\r\n",
        cpu::hart_id(),
    ));
}


pub fn init_hart() {
    assert_eq!(irq::enabled(), false);

    let entry = _trap_entry as *const () as usize;

    assert_eq!(entry & 0x3, 0);

    let actual: usize;

    unsafe {
        asm!(
            "csrw stvec, {entry}",
            "csrr {actual}, stvec",
            entry = in(reg) entry,
            actual = out(reg) actual,
            options(nostack),
        );
    }

    assert_eq!(actual, entry);
}


#[unsafe(no_mangle)]
pub extern "C" fn rust_trap_fatal(
    scause: usize,
    sepc: usize,
    stval: usize,
    sstatus: usize,
    original_sp: usize,
) -> ! {
    if !TRAP_REPORTED.swap(true, Ordering::Relaxed) {
        let interrupt_bit = 1usize << 63;
        let is_interrupt = scause & interrupt_bit != 0;
        let code = scause & !interrupt_bit;

        let mut writer = uart::Writer::new();

        let _ = write!(
            &mut writer,
            concat!(
                "TRAP hart={}, interrupt={}, code={}\r\n",
                "scause={:#x}, sepc={:#x}, stval={:#x}, sstatus={:#x}, sp={:#x}\r\n",
            ),
            cpu::hart_id(),
            is_interrupt,
            code,
            scause,
            sepc,
            stval,
            sstatus,
            original_sp,
        );
    }

    loop {
        core::hint::spin_loop();
    }
}

#[repr(C)]
pub struct TrapFrame {
    pub x: [usize; 32],
    pub sepc: usize,
    pub sstatus: usize,
}


const _: () = {
    assert!(core::mem::size_of::<TrapFrame>() == 272);
    assert!(core::mem::offset_of!(TrapFrame, sepc) == 256);
    assert!(core::mem::offset_of!(TrapFrame, sstatus) == 264);
};


/// 接收汇编入口保存的 S 模式整数寄存器现场
/// 
/// 汇编入口保证 frame 指向当前内核栈中已完整初始化、独占访问的现场
#[unsafe(no_mangle)]
pub extern "C" fn rust_trap(frame: &mut TrapFrame) {
    let scause: usize;
    let stval: usize;

    unsafe {
        asm!(
            "csrr {cause}, scause",
            "csrr {value}, stval",
            cause = out(reg) scause,
            value = out(reg) stval,
            options(nostack),
        );
    }

    assert!(
        frame.sstatus & (1usize << 8) != 0,
        "trap: expected S-mode origin",
    );

    assert!(
        !irq::enabled(),
        "trap: interrupts must remain disabled",
    );

    let timer_interrupt = (1usize << 63) | 5;

    if scause == timer_interrupt {
        crate::timer::on_interrupt();
        return;
    }

    let external_interrupt = (1usize << 63) | 9;

    if scause == external_interrupt {
        handle_external_interrupt();
        return;
    }

    #[cfg(feature = "trap-test")]
    {
        let test_pc = _trap_test_breakpoint as *const () as usize;

        if scause == 3 && frame.sepc == test_pc {
            frame.sepc += 4;
            return;
        }
    }

    rust_trap_fatal(
        scause,
        frame.sepc,
        stval,
        frame.sstatus,
        frame.x[2],
    )
}


fn handle_external_interrupt() {
    let Some(claim) = plic::claim() else {
        return;
    };

    let irq = claim.irq();
    if irq == uart::UART_IRQ {
        input::receive_interrupt();
    } else {
        drop(claim);
        panic!("plic: unexpected IRQ {}", irq);
    }
}