use super::{
    PAGE_SIZE, RAM_END,
    kernel_layout, pagetable, pmem,
    vm::{PTE_R, PTE_W, PTE_X},
};
use crate::uart::UART_BASE;


pub fn build() -> usize {
    let layout = kernel_layout();
    let mut root = pmem::alloc_kernel();

    let regions = [
        (layout.start, layout.text_end, PTE_R | PTE_X),
        (layout.text_end, layout.rodata_end, PTE_R),
        (layout.rodata_end, RAM_END, PTE_R | PTE_W),
        (UART_BASE, UART_BASE + PAGE_SIZE, PTE_R | PTE_W),
    ];

    for (start, end, permissions) in regions {
        assert!(start < end);
        assert!(start % PAGE_SIZE == 0 && end % PAGE_SIZE == 0);

        for pa in (start..end).step_by(PAGE_SIZE) {
            // SAFETY 根页表刚从分配器获得，下级页表由构建过程创建；
            // 整棵树尚未发布、未被硬件使用，当前 hart 独占修改
            unsafe { 
                pagetable::map_4k(root, pa, pa, permissions) 
                    .expect("kernel mapping failed")
            };
        }
    }

    root
}