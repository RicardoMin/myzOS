use super::{
    PAGE_SIZE, RAM_END,
    kernel_layout, pagetable, pmem,
    vm::{PTE_R, PTE_W, PTE_X},
};
use super::layout::{
    KSTACK_BOTTOM,
    KSTACK_TOP,
};
use crate::{irq, uart::UART_BASE};
use crate::plic::{PLIC_BASE, PLIC_SIZE};
use core::arch::asm;


pub fn build() -> usize {
    let layout = kernel_layout();
    let root = pmem::alloc_kernel();

    let regions = [
        (layout.start, layout.text_end, PTE_R | PTE_X),
        (layout.text_end, layout.rodata_end, PTE_R),
        (layout.rodata_end, RAM_END, PTE_R | PTE_W),
        (UART_BASE, UART_BASE + PAGE_SIZE, PTE_R | PTE_W),
        (PLIC_BASE, PLIC_BASE + PLIC_SIZE, PTE_R | PTE_W),
    ];

    for (start, end, permissions) in regions {
        assert!(start < end);
        assert!(start % PAGE_SIZE == 0 && end % PAGE_SIZE == 0);

        for pa in (start..end).step_by(PAGE_SIZE) {
            // SAFETY: 根页表刚从分配器获得，下级页表由构建过程创建；
            // 整棵树尚未发布、未被硬件使用，当前 hart 独占修改
            unsafe { 
                pagetable::map_4k(root, pa, pa, permissions) 
                    .expect("kernel mapping failed")
            };
        }
    }

    for va in (KSTACK_BOTTOM..KSTACK_TOP).step_by(PAGE_SIZE) {
        let pa = pmem::alloc_kernel();

        unsafe {
            pagetable::map_4k(root, va, pa, PTE_R | PTE_W)
                .expect("cannot map kernel stack");
        };
    }

    root
}


pub fn read_satp() -> usize {
    let value: usize;

    unsafe {
        asm!(
            "csrr {value}, satp",
            value = out(reg) value,
            options(nostack),
        );
    }

    value
}


/// # Safety
/// root_pa 指向完整、有效的 Sv39 页表
/// 新映射必须保持继续执行所需的代码、栈、数据和设备的地址及适当权限
/// 页表页在使用期间持续存活：本阶段发布后不得修改或释放
pub unsafe fn enable(root_pa: usize) {
    assert!(root_pa % PAGE_SIZE == 0);
    assert!(root_pa >> 56 == 0);
    assert_eq!(read_satp(), 0);
    assert_eq!(irq::enabled(), false);

    let value = (root_pa >> 12) | (8usize << 60);

    unsafe {
        asm!(
            "sfence.vma zero, zero",
            "csrw satp, {value}",
            "sfence.vma zero, zero",
            value = in(reg) value,
            options(nostack),
        );
    }

    assert_eq!(read_satp(), value);
}