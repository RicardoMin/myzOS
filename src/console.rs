use core::fmt::{self, Write};

use crate::{cpu, spinlock::RawSpinLock, uart};

static CONSOLE_LOCK: RawSpinLock = RawSpinLock::new();

pub fn print(args: fmt::Arguments<'_>) -> fmt::Result {
    let _guard = CONSOLE_LOCK.lock();

    cpu::io_fence();

    let mut writer = uart::Writer::new();

    let result = writer.write_fmt(args);

    cpu::io_fence();

    result
}

pub fn echo_byte(byte: u8) {
    let _guard = CONSOLE_LOCK.lock();
    cpu::io_fence();
    let mut writer = uart::Writer::new();

    match byte {
        b'\r' | b'\n' => {
            writer.put_byte(b'\r');
            writer.put_byte(b'\n');
        }
        b' '..=b'~' => writer.put_byte(byte),
        _ => {}
        }
    cpu::io_fence();
}