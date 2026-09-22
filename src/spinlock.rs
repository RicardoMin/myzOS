use core::sync::atomic::{AtomicBool, Ordering};


pub struct RawSpinLock {
    locked: AtomicBool,
}


impl RawSpinLock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    pub fn try_lock(&self) -> bool {
        self.locked
            .compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    pub fn lock(&self) {
        while !self.try_lock() {
            core::hint::spin_loop();
        }
    }

    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}