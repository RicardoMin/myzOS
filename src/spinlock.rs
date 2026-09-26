use core::sync::atomic::{AtomicBool, Ordering};


pub struct RawSpinLock {
    locked: AtomicBool,
}


#[must_use = "keep the guard alive while accessing protected data"]
pub struct RawSpinLockGuard<'a> {
    lock: &'a RawSpinLock,
}


impl RawSpinLock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    pub fn try_acquire(&self) -> bool {
        self.locked
            .compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    pub fn lock(&self) -> RawSpinLockGuard<'_> {
        while !self.try_acquire() {
            core::hint::spin_loop();
        }

        RawSpinLockGuard { lock: self }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}


impl Drop for RawSpinLockGuard<'_> {
    fn drop(&mut self) {
        self.lock.unlock();
    }
}