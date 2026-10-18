use crate::{spinlock::SpinLock, uart};

const RX_CAPACITY: usize = 128;

struct RxQueue {
    buf: [u8; RX_CAPACITY],
    read: usize,
    len: usize,
    dropped: usize,
}

impl RxQueue {
    const fn new() -> Self {
        Self {
            buf: [0; RX_CAPACITY],
            read: 0,
            len: 0,
            dropped: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        if self.len == RX_CAPACITY {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }

        let write = (self.read + self.len) % RX_CAPACITY;
        self.buf[write] = byte;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }

        let byte = self.buf[self.read];
        self.read = (self.read + 1) % RX_CAPACITY;
        self.len -= 1;

        Some(byte)
    }
}


static RX: SpinLock<RxQueue> = SpinLock::new(RxQueue::new());

pub fn pop() -> Option<u8> {
    RX.lock().pop()
}

pub fn is_empty() -> bool {
    RX.lock().len == 0
}

pub fn take_dropped() -> usize {
    let mut guard = RX.lock();
    let dropped = guard.dropped;
    guard.dropped = 0;
    dropped
}

pub fn receive_interrupt() {
    let mut queue = RX.lock();

    for _ in 0..32 {
        let Some(byte) = uart::try_read() else {
            break;
        };

        queue.push(byte)
    }
}