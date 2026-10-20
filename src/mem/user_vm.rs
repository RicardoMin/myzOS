use core::{ptr, slice};

use super::{
    PAGE_SIZE, pagetable, pmem,
    vm::{PTE_R, PTE_W, PTE_X, PTE_U},
};
use crate::console;

pub const USER_ENTRY: usize = 0x1000;
pub const USER_STACK_BOTTOM: usize = (1usize << 38) - 3 * PAGE_SIZE;
pub const USER_STACK_TOP: usize = USER_STACK_BOTTOM + PAGE_SIZE;

pub struct UserSpace {
    root_pa: usize,
    image_pa: usize,
    stack_pa: usize,
}

impl UserSpace {
    pub fn new(image: &[u8]) -> Self {
        assert!(!image.is_empty());
        assert!(image.len() <= PAGE_SIZE);

        let root_pa = pmem::alloc_kernel();
        let image_pa = pmem::alloc_data();
        let stack_pa = pmem::alloc_data();

        unsafe {
            ptr::copy_nonoverlapping(
                image.as_ptr(),
                image_pa as *mut u8,
                image.len(),
            );

            pagetable::map_4k(
                root_pa,
                USER_ENTRY,
                image_pa,
                PTE_R | PTE_W | PTE_X | PTE_U,
            )
            .expect("cannot map user iamge");

            pagetable::map_4k(
                root_pa,
                USER_STACK_BOTTOM,
                stack_pa,
                PTE_R | PTE_W | PTE_U,
            )
            .expect("cannot map user stack");
        }

        Self {
            root_pa,
            image_pa,
            stack_pa,
        }
    }

    /// # Safety
    /// 页表从未被任何 hart 启用；独占所有页面，所有借用和访问已经结束
    /// 映射仍由本对象管理，未被外部修改
    pub unsafe fn destroy(self) {
        let old = unsafe {
            pagetable::unmap_4k(self.root_pa, USER_ENTRY)
                .expect("user image missing")
        };

        assert_eq!(old.pa(), self.image_pa);

        let old_stack = unsafe {
            pagetable::unmap_4k(self.root_pa, USER_STACK_BOTTOM)
                .expect("user stack missing")
        };

        unsafe {
            pmem::free_data(self.image_pa);
            pmem::free_data(self.stack_pa);
            pmem::free_kernel(self.root_pa);
        }
    }
}


pub fn check() {
    let image: &[u8] = include_bytes!(env!("MYZOS_USER_IMAGE"));
    let before = pmem::free_counts();
    let space = UserSpace::new(image);

    // SAFETY: 当前独占整个地址空间，页表从未交给硬件
    unsafe {
        for (va, pa, permissions) in [
             (
                USER_ENTRY,
                space.image_pa,
                PTE_R | PTE_W | PTE_X | PTE_U,
            ),
            (
                USER_STACK_BOTTOM,
                space.stack_pa,
                PTE_R | PTE_W | PTE_U,
            ),
        ] {
            let pte = pagetable::lookup_4k(space.root_pa, va)
                .expect("user mapping missing");

            assert_eq!(pte.pa(), pa);
            assert_eq!(
                pte.flags() & (PTE_R | PTE_W | PTE_X | PTE_U),
                permissions,
            );
        }

        {
            let copied = slice::from_raw_parts(
                space.image_pa as *const u8,
                PAGE_SIZE,
            );
            assert_eq!(&copied[..image.len()], image);
            assert!(copied[image.len()..].iter().all(|&byte| byte == 0));

            let stack = slice::from_raw_parts(
                space.stack_pa as *const u8,
                PAGE_SIZE,
            );
            assert!(stack.iter().all(|&byte| byte == 0));
        }

        for va in [
            0,
            0x8020_0000,
            USER_STACK_TOP,
            USER_STACK_TOP + PAGE_SIZE,
        ] {
            assert!(pagetable::lookup_4k(space.root_pa, va).is_none());
        }

        space.destroy();
    }

    assert_eq!(pmem::free_counts(), before);

    let _ = console::print(format_args!(
        "user-vm: image={} bytes, mapping and reclaim checks passed (not active)\r\n",
        image.len(),
    ));
}