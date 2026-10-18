use core::fmt;

use crate::cpu;

pub(crate) const UART_BASE: usize = 0x1000_0000;
pub(crate) const UART_IRQ: u32 = 10;

const THR: usize = 0;
const LSR: usize = 5;
const LSR_THRE: u8 = 1 << 5;

const RBR: usize = 0;
const IER: usize = 1;
const LCR: usize = 3;

const LSR_DR: u8 = 1 << 0;
const LCR_DLAB: u8 = 1 << 7;
const IER_RX: u8 = 1 << 0;

pub struct Writer;

fn read8(offset: usize) -> u8 {
    cpu::io_fence();
    let res = unsafe{ core::ptr::read_volatile((UART_BASE + offset) as *const u8) };
    cpu::io_fence();

    res
}

fn write8(offset: usize, val: u8) {
    cpu::io_fence();
    unsafe { core::ptr::write_volatile((UART_BASE + offset) as *mut u8, val); }
    cpu::io_fence();
}

impl Writer {
    pub const fn new() -> Self {
        Self
    }

    pub(crate)fn put_byte(&mut self, byte: u8) {
        while (read8(LSR) & LSR_THRE) == 0 {}

        write8(THR, byte);
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            self.put_byte(byte);
        }

        Ok(())
    }
}


pub fn enable_rx_interrupt() {
    let mut lcr = read8(LCR);
    lcr &= !LCR_DLAB;
    write8(LCR, lcr);

    write8(IER, IER_RX);
}


pub fn try_read() -> Option<u8> {
    if (read8(LSR) & LSR_DR) == 0 {
        return None;
    }

    Some(read8(RBR))
}