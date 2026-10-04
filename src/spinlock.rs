use core::sync::atomic::{AtomicBool, Ordering};
use core::marker::PhantomData;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

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


pub struct SpinLock<T> {
    raw: RawSpinLock,
    value: UnsafeCell<T>,
}

#[must_use = "keep the guard alive while accessing protected data"]
pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    _raw: RawSpinLockGuard<'a>,
}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            raw: RawSpinLock::new(),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let raw = self.raw.lock();
        SpinLockGuard {
            lock: self,
            _raw: raw,
        }

    }
}

// SAFETY: 内部数据只能通过持锁护卫访问；
// T: Send 允许数据的独占访问权在不同执行者之间交接
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: 此守卫持有原始锁；
        // 返回引用的有效期受当前守卫借用约束
        unsafe {
            &*self.lock.value.get()
        }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    // SAFETY: 当前守卫持锁；
    // 返回的可变引用受当前守卫的可变借用约束
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *self.lock.value.get()
        }
    }
}