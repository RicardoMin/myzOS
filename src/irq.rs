use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::cpu;


const SSTATUS_SIE: usize = 1 << 1;

struct IrqState {
    depth: AtomicUsize,
    was_enabled: AtomicBool,
}

impl IrqState {
    const fn new() -> Self {
        Self {
            depth: AtomicUsize::new(0),
            was_enabled: AtomicBool::new(false),
        }
    }
}

static IRQ_STATES: [IrqState; cpu::MAX_HARTS] = [
    IrqState::new(),
    IrqState::new(),
];

fn current_state() -> &'static IrqState {
    let id = cpu::hart_id();

    assert!(id < cpu::MAX_HARTS);

    &IRQ_STATES[id]
}

pub(crate) fn depth() -> usize {
    current_state().depth.load(Ordering::Relaxed)
}

pub(crate) fn push_off() {
    let old_enabled = save_and_disable();
    let state = current_state();
    let old_depth = state.depth.load(Ordering::Relaxed);

    if old_depth == 0 {
        state.was_enabled.store(old_enabled, Ordering::Relaxed);
    } else {
        assert_eq!(old_enabled, false);
    }

    let next = old_depth
        .checked_add(1)
        .expect("IRQ depth overflow");
    state.depth.store(next, Ordering::Relaxed);
}

/// # Safety
/// 必须在同一 hart 上，与此前尚未配对的 push_off 对应；
/// 调用时仍处于关中断状态
pub(crate) unsafe fn pop_off() {
    assert_eq!(enabled(), false);
    let state = current_state();
    let old_depth = state.depth.load(Ordering::Relaxed);
    assert!(old_depth > 0);

    let next = old_depth - 1;
    state.depth.store(next, Ordering::Relaxed);

    if next == 0 {
        let was_enabled = state.was_enabled.load(Ordering::Relaxed);
        state.was_enabled.store(false, Ordering::Relaxed);

        if was_enabled {
            unsafe {
                restore(true);
            }
        }
    }
}

pub fn enabled() -> bool {
    let status: usize;

    unsafe {
        asm!(
            "csrr {status}, sstatus",
            status = out(reg) status,
            options(nostack),
        );
    }

    status & SSTATUS_SIE != 0
}


pub fn save_and_disable() -> bool {
    let old: usize;

    unsafe {
        asm!(
            "csrrc {old}, sstatus, {mask}",
            old = out(reg) old,
            mask = in(reg) SSTATUS_SIE,
            options(nostack),
        );
    }

    old & SSTATUS_SIE != 0
}


/// # Safety
/// 恢复状态必须属于当前 hart 的对应保存操作；
/// 不能绕过仍然有效的临界区提前开启中断，开启前必须具备处理中断的条件
pub unsafe fn restore(was_enabled: bool) {
    if was_enabled {
        unsafe {
            asm!(
                "csrs sstatus, {mask}",
                mask = in(reg) SSTATUS_SIE,
                options(nostack),
            );
        }
    } else {
        unsafe {
            asm!(
                "csrc sstatus, {mask}",
                mask = in(reg) SSTATUS_SIE,
                options(nostack),
            )
        }
    }
}


/// 开启当前 hart 的 S 模式中断
/// 
/// # Safety
/// 当前 hart 必须已安装有效异常入口，并具备有效内核栈；
/// 所有已是能的中断来源都必须有可用的处理逻辑
/// 调用时不得处于尚未结束的关中断临界区
pub unsafe fn enable() {
    assert_eq!(depth(), 0);

    unsafe {
        asm!(
            "csrs sstatus, {mask}",
            mask = in(reg) SSTATUS_SIE,
            options(nostack),
        );
    }
}