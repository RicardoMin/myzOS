use core::sync::atomic::{AtomicBool, Ordering};
use core::marker::PhantomData;

use crate::{cpu, irq};


pub struct RawSpinLock {
    locked: AtomicBool,
}


#[must_use = "keep the guard alive while accessing protected data"]
pub struct RawSpinLockGuard<'a> {
    lock: &'a RawSpinLock,
    hart_id: usize,
    _not_send: PhantomData<*mut ()>,
}


impl RawSpinLock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    fn try_acquire(&self) -> bool {
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
        irq::push_off();

        while !self.try_acquire() {
            core::hint::spin_loop();
        }

        RawSpinLockGuard {
            lock: self,
            hart_id: cpu::hart_id(),
            _not_send: PhantomData,
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}


impl Drop for RawSpinLockGuard<'_> {
    fn drop(&mut self) {
        assert_eq!(cpu::hart_id(), self.hart_id);
        assert!(!irq::enabled());

        self.lock.unlock();

        unsafe {
            irq::pop_off();
        }
    }
}