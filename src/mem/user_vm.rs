use core::{ptr, slice};
use super::{
    PAGE_SIZE, pagetable, pmem,
    vm::{PTE_R, PTE_W, PTE_X, PTE_U},
};
use crate::user_frame::UserFrame;
use super::layout::{
    USER_ENTRY,
    USER_STACK_BOTTOM,
    USER_STACK_TOP,
    USER_FRAME,
    TRAMPOLINE,
    KSTACK_BOTTOM,
    SV39_LOW_END,
};
use crate::trampoline;


#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UserCopyError {
    InvalidRange,
    NotReadable,
}

pub struct UserSpace {
    root_pa: usize,
    image_pa: usize,
    stack_pa: usize,
    frame_pa: usize,
}

impl UserSpace {
    pub fn new(image: &[u8]) -> Self {
        assert!(!image.is_empty());
        assert!(image.len() <= PAGE_SIZE);

        let root_pa = pmem::alloc_kernel();
        let image_pa = pmem::alloc_data();
        let stack_pa = pmem::alloc_data();
        let frame_pa = pmem::alloc_kernel();

        unsafe {
            (frame_pa as *mut UserFrame).write(
                UserFrame::new(USER_ENTRY, USER_STACK_TOP),
            );

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
            .expect("cannot map user image");

            pagetable::map_4k(
                root_pa,
                USER_STACK_BOTTOM,
                stack_pa,
                PTE_R | PTE_W | PTE_U,
            )
            .expect("cannot map user stack");

            pagetable::map_4k(
                root_pa,
                USER_FRAME,
                frame_pa,
                PTE_R | PTE_W,
            )
            .expect("cannot map user frame");

            pagetable::map_4k(
                root_pa,
                TRAMPOLINE,
                trampoline::pa(),
                PTE_R | PTE_X,
            )
            .expect("cannot map trampoline");

        }

        Self {
            root_pa,
            image_pa,
            stack_pa,
            frame_pa,
        }
    }

    /// # Safety
    /// 页表从未被任何 hart 启用；页表及私有页由调用者独占，所有借用和访问已结束
    /// 映射未被外部修改, trampoline 是永久只读共享页，本对象不拥有它
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

        assert_eq!(old_stack.pa(), self.stack_pa);

        let old_frame = unsafe {
            pagetable::unmap_4k(self.root_pa, USER_FRAME)
                .expect("user frame missing")
        };

        assert_eq!(old_frame.pa(), self.frame_pa);

        let old_trampoline = unsafe {
            pagetable::unmap_4k(self.root_pa, TRAMPOLINE)
                .expect("trampoline missing")
        };

        assert_eq!(old_trampoline.pa(), trampoline::pa());

        unsafe {
            pmem::free_data(self.image_pa);
            pmem::free_data(self.stack_pa);
            pmem::free_kernel(self.frame_pa);
            pmem::free_kernel(self.root_pa);
        }
    }

    pub(crate) fn satp(&self) -> usize {
        (8usize << 60) | (self.root_pa >> 12)
    }

    pub(crate) fn frame_pa(&self) -> usize {
        self.frame_pa
    }

    /// # Safety
    /// 当前使用可访问物理页的内核恒等映射；用户执行已暂停
    /// 页表和源页持续有效，无并发修改；dst 不与源物理区域重叠
    pub(crate) unsafe fn copy_from_user(
        &self,
        dst: &mut [u8],
        src: usize,
    ) -> Result<(), UserCopyError> {
        if dst.is_empty() {
            return Ok(());
        }
        let end = src
            .checked_add(dst.len())
            .ok_or(UserCopyError::InvalidRange)?;

        if end > SV39_LOW_END {
            return Err(UserCopyError::InvalidRange);
        }

        let mut copied = 0;

        while copied < dst.len() {
            let va = src + copied;
            let offset = super::vm::page_offset(va);

            let pte = unsafe {
                pagetable::lookup_4k(self.root_pa, va)
            }
            .ok_or(UserCopyError::NotReadable)?;

            let required = PTE_R | PTE_U;
            if pte.flags() & required != required {
                return Err(UserCopyError::NotReadable);
            }

            let n = core::cmp::min(
                PAGE_SIZE - offset,
                dst.len() - copied,
            );

            unsafe {
                ptr::copy_nonoverlapping(
                    (pte.pa() + offset) as *const u8,
                    dst.as_mut_ptr().add(copied),
                    n,
                );
            }

            copied += n;
        }

        Ok(())
    }

    /// # Safety
    /// 页表从未启用，页表及私有页由调用者独占且存活，映射未被外部修改
    /// 共享 trampoline 页持续有效且不可修改
    pub(crate) unsafe fn check_layout(&self, image: &[u8]) {
        // SAFETY: 当前独占整个地址空间，页表从未交给硬件
        unsafe {
            for (va, pa, permissions) in [
                (
                    USER_ENTRY,
                    self.image_pa,
                    PTE_R | PTE_W | PTE_X | PTE_U,
                ),
                (
                    USER_STACK_BOTTOM,
                    self.stack_pa,
                    PTE_R | PTE_W | PTE_U,
                ),
                (
                    USER_FRAME,
                    self.frame_pa,
                    PTE_R | PTE_W,
                ),
                (
                    TRAMPOLINE,
                    trampoline::pa(),
                    PTE_R | PTE_X,
                ),
            ] {
                let pte = pagetable::lookup_4k(self.root_pa, va)
                    .expect("user mapping missing");

                assert_eq!(pte.pa(), pa);
                assert_eq!(
                    pte.flags() & (PTE_R | PTE_W | PTE_X | PTE_U),
                    permissions,
                );
            }

            {
                let copied = slice::from_raw_parts(
                    self.image_pa as *const u8,
                    PAGE_SIZE,
                );
                assert_eq!(&copied[..image.len()], image);
                assert!(copied[image.len()..].iter().all(|&byte| byte == 0));

                let stack = slice::from_raw_parts(
                    self.stack_pa as *const u8,
                    PAGE_SIZE,
                );
                assert!(stack.iter().all(|&byte| byte == 0));

                let frame = &*(self.frame_pa as *const UserFrame);

                assert_eq!(frame.regs.sepc, USER_ENTRY);
                assert_eq!(frame.regs.x[2], USER_STACK_TOP);
            }

            for va in [
                0,
                0x8020_0000,
                KSTACK_BOTTOM,
            ] {
                assert!(pagetable::lookup_4k(self.root_pa, va).is_none());
            }

            pagetable::map_4k(self.root_pa, 0x2000, self.image_pa, PTE_R | PTE_U)
                .expect("map cross-page test alias failed");

            let mut actual = [0u8; 4];

            self.copy_from_user(&mut actual, 0x1ffe)
                .expect("cross-page user copy failed");
            
            let expected = [
                image.get(PAGE_SIZE -2).copied().unwrap_or(0),
                image.get(PAGE_SIZE - 1).copied().unwrap_or(0),
                image.first().copied().unwrap_or(0),
                image.get(1).copied().unwrap_or(0),
            ];

            assert_eq!(
                actual,
                expected,
                "cross-page user copy returned incorrect bytes",
            );

            let pte = pagetable::unmap_4k(self.root_pa, 0x2000)
                .expect("unmap 0x2000 failed");
            assert_eq!(pte.pa(), self.image_pa);
        }
    }
}