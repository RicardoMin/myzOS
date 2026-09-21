const UART_BASE: usize = 0x1000_0000;

const THR: usize = 0;
const LSR: usize = 5;
const LSR_THRE: u8 = 1 << 5;

pub fn put_byte(byte: u8) {
    unsafe {
        while (core::ptr::read_volatile((UART_BASE + LSR) as *const u8) & LSR_THRE) == 0 {}

        core::ptr::write_volatile((UART_BASE + THR) as *mut u8, byte);
    }
}
