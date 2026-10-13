use core::arch::asm;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{console, cpu, irq, uart};

core::arch::global_asm!(include_str!("trap.S"));

unsafe extern "C" {
    fn _trap_entry();
}

static TRAP_REPORTED: AtomicBool = AtomicBool::new(false);


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
                "TRAP hart={}, interrupt={}, code={:#x}\r\n",
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