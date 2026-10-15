use core::ptr::{read_volatile, write_volatile};

use crate::{cpu, irq, uart::UART_IRQ};


pub(crate) const PLIC_BASE: usize = 0x0c00_0000;
pub(crate) const PLIC_SIZE: usize = 0x0400_0000;


fn read32(offset: usize) -> u32 {
    cpu::io_fence();
    let res = unsafe{ read_volatile((PLIC_BASE + offset) as *const u32) };
    cpu::io_fence();

    res
}


fn write32(offset: usize, value: u32) {
    cpu::io_fence();
    unsafe { write_volatile((PLIC_BASE + offset) as *mut u32, value); }
    cpu::io_fence();
}


pub fn init_uart(boot_hart: usize) {
    assert!(boot_hart < cpu::MAX_HARTS);
    assert!(!irq::enabled());

    for hart in 0..cpu::MAX_HARTS {
        let context = 2 * hart + 1;
        let enable_base = 0x2000 + context * 0x80;

        for word in 0..32 {
            write32(enable_base + word * 4, 0);
        }

        let threshold = 0x20_0000 + context * 0x1000;
        write32(threshold, 0);
    }

    write32(4 * UART_IRQ as usize, 1);

    let context = 2 * boot_hart + 1;
    let word = UART_IRQ as usize / 32;
    let mask = 1u32 << (UART_IRQ % 32);
    let enable = 0x2000 + context * 0x80 + word * 4;

    write32(enable, mask);
}


pub struct Claim {
    irq: u32,
    register: *mut u32,
}

impl Claim {
    pub fn irq(&self) -> u32 {
        self.irq
    }
}

pub fn claim() -> Option<Claim> {
    let context = cpu::hart_id() * 2 + 1;
    let register_offset = 0x20_0000 + context * 0x1000 + 0x4;
    let irq = read32(register_offset);

    if irq == 0 {
        None
    } else {
        Some(Claim { irq, register: (PLIC_BASE + register_offset) as *mut u32})
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        cpu::io_fence();
        unsafe {
            write_volatile(self.register, self.irq);
        }
        cpu::io_fence();
    }
}