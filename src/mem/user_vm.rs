use core::{ptr, slice};
use super::{
    PAGE_SIZE, pagetable, pmem,
    vm::{Pte, PTE_R, PTE_W, PTE_X, PTE_U},
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
    NotWritable,
    Unterminated,
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
    /// 动态映射已经清空，仅保留构造时建立的固定映射
    pub unsafe fn destroy(self) {
        let old_frame = unsafe {
            pagetable::unmap_4k(self.root_pa, USER_FRAME)
                .expect("user frame missing")
        };

        assert_eq!(old_frame.pa(), self.frame_pa);
        unsafe{pmem::free_kernel(self.frame_pa);}

        let old_trampoline = unsafe {
            pagetable::unmap_4k(self.root_pa, TRAMPOLINE)
                .expect("trampoline missing")
        };

        assert_eq!(old_trampoline.pa(), trampoline::pa());

        unsafe {
            Self::destroy_table(self.root_pa, 2);
        }
    }

    /// # Safety
    /// 表树从未启用且由调用者独占，无共享子表、环或存活借用
    /// 特殊映射已摘除，每个有效叶独占一个 4 KiB 用户数据池页
    /// 各级表均来自内核页池且可直接访问，调用结束后 table_pa 已释放
    unsafe fn destroy_table(table_pa: usize, level: usize) {
        for index in 0..pagetable::ENTRIES {
            let entry = unsafe{
                pagetable::read_pte(table_pa, index)
            };
            if !entry.is_valid() {
                continue;
            }

            if entry.is_leaf() {
                assert_eq!(level, 0);
                assert!(entry.flags() & PTE_U == PTE_U);
                unsafe {
                    pagetable::write_pte(table_pa, index, Pte::empty());
                    pmem::free_data(entry.pa());
                }
            } else {
                assert!(level > 0);
                unsafe {
                    pagetable::write_pte(table_pa, index, Pte::empty());
                    Self::destroy_table(entry.pa(), level - 1);
                }
            }
        }
        unsafe {
            pmem::free_kernel(table_pa);
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
    /// 用户执行已暂停，页表和相关物理页存活且稳定，内核恒等映射可写
    /// 目标物理区域无并发访问或冲突引用，且不与 src 重叠
    pub(crate) unsafe fn copy_to_user(
        &self,
        dst: usize,
        src: &[u8],
    ) -> Result<(), UserCopyError> {
        if src.is_empty() {
            return Ok(());
        }

        let end = dst
            .checked_add(src.len())
            .ok_or(UserCopyError::InvalidRange)?;

        if end > SV39_LOW_END {
            return Err(UserCopyError::InvalidRange);
        }

        let mut copied = 0;
        while copied < src.len() {
            let va = dst + copied;
            let offset = super::vm::page_offset(va);

            let pte = unsafe {
                pagetable::lookup_4k(self.root_pa, va)
            }
            .ok_or(UserCopyError::NotWritable)?;
            
            let required = PTE_U | PTE_W;
            if (pte.flags() & required) != required {
                return Err(UserCopyError::NotWritable);
            }

            let n = core::cmp::min(
            PAGE_SIZE - offset,
            src.len() - copied,
            );

            unsafe {
                ptr::copy_nonoverlapping(
                    src.as_ptr().add(copied),
                    (pte.pa() + offset) as *mut u8,
                    n,
                );
            }

            copied += n;
        }

        Ok(())
    }

    /// 复制包含 NUL 的用户字符串，返回不含 NUL 的字节长度
    ///
    /// # Safety
    /// 用户执行已停止；页表和源页存活且无并发修改，内核恒等映射可读
    /// dst 不与源物理区重叠，且不存在冲突引用
    pub(crate) unsafe fn copy_str_from_user(
        &self,
        dst: &mut [u8],
        src: usize,
    ) -> Result<usize, UserCopyError> {
        let mut copied = 0;

        while copied < dst.len() {
            let va = src
                .checked_add(copied)
                .ok_or(UserCopyError::InvalidRange)?;

            if va >= SV39_LOW_END {
                return Err(UserCopyError::InvalidRange);
            }

            let pte = unsafe {
                pagetable::lookup_4k(self.root_pa, va)
                    .ok_or(UserCopyError::NotReadable)?
            };

            let required = PTE_U | PTE_R;

            if (pte.flags() & required) != required {
                return Err(UserCopyError::NotReadable);
            }

            let offset = super::vm::page_offset(va);
            let n = core::cmp::min(
                PAGE_SIZE - offset,
                dst.len() - copied
            );

            for i in 0..n {
                let byte = unsafe {
                    ptr::read((pte.pa() + offset + i) as *const u8)
                };

                dst[copied + i] = byte;

                if byte == 0 {
                    return Ok(copied + i);
                }
            }

            copied += n;
        }
        Err(UserCopyError::Unterminated)
    }

    /// # Safety
    /// 新旧堆顶合法，old_top 准确描述连续、独占的堆页
    /// 增长和收缩区间分别满足对应页面区域操作的安全条件
    pub(crate) unsafe fn resize_heap(
        &mut self,
        old_top: usize,
        new_top: usize,
    ) {
        if new_top > old_top {
            unsafe {
                self.map_user_range(old_top, new_top);
            }
        } else {
            unsafe {
                self.unmap_user_range(new_top, old_top);
            }
        }
    }

    /// # Safety
    /// 区间页面对齐且属于合法用户区域，目标区间尚未映射
    /// 页表独占且存活，未启用或已切离并完成同步；恢复使用前须同步新映射
    pub(crate) unsafe fn map_user_range(
        &mut self,
        begin: usize,
        end: usize,
    ) {
        for va in (begin..end).step_by(PAGE_SIZE) {
            let pa = pmem::alloc_data();
            unsafe {
                pagetable::map_4k(self.root_pa, va, pa, PTE_U | PTE_R | PTE_W)
                    .expect("data map failed")
            };
        }
    }

    /// # Safety
    /// 区间页对齐，连续映射本地址空间独占的数据池页，无存活引用或访问
    /// 页表独占且存活，未启用或已切离并完成同步；恢复使用前须同步页表修改
    pub(crate) unsafe fn unmap_user_range(
        &mut self,
        begin: usize,
        end: usize,
    ) {
        for va in (begin..end).step_by(PAGE_SIZE) {
            unsafe {
                let pte = pagetable::unmap_4k(self.root_pa, va)
                    .expect("data unmap failed");
                pmem::free_data(pte.pa());
            }
        }
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

            let before = ptr::read(self.image_pa as *const u8);

            assert_eq!(
                self.copy_to_user(0x2000, &[before ^ 0xff]),
                Err(UserCopyError::NotWritable),
            );

            assert_eq!(
                ptr::read(self.image_pa as *const u8),
                before,
            );

            let pte = pagetable::unmap_4k(self.root_pa, 0x2000)
                .expect("unmap 0x2000 failed");
            assert_eq!(pte.pa(), self.image_pa);

            pagetable::map_4k(self.root_pa, 0x2000, self.stack_pa, PTE_R | PTE_W | PTE_U)
                .expect("map writable test alias failed");

            self.copy_to_user(0x1ffe, &[0x11, 0x22, 0x33, 0x44])
                .expect("cross-page user write failed");

            let actual = [
                ptr::read((self.image_pa + PAGE_SIZE - 2) as *const u8),
                ptr::read((self.image_pa + PAGE_SIZE - 1) as *const u8),
                ptr::read(self.stack_pa as *const u8),
                ptr::read((self.stack_pa + 1) as *const u8),
            ];

            assert_eq!(
                actual,
                [0x11, 0x22, 0x33, 0x44],
                "cross-page user write reached incorrect physical locations",
            );

            self.copy_to_user(0x1ffe, b"OS\0X")
                .expect("write user string failed");

            let mut crossing = [0xccu8; 3];
            let ret1= self.copy_str_from_user(&mut crossing, 0x1ffe);
            assert_eq!(ret1, Ok(2));
            assert_eq!(&crossing, b"OS\0");

            let pte = pagetable::unmap_4k(self.root_pa, 0x2000)
                .expect("unmap writable test alias failed");

            assert_eq!(
                pte.pa(),
                self.stack_pa,
                "writable test alias pointed to an unexpected physical page",
            );

            self.copy_to_user(0x1ffe, b"Z\0")
                .expect("write user string failed");

            let mut ending = [0xccu8; 8];
            let ret2 = self.copy_str_from_user(&mut ending, 0x1ffe);
            assert_eq!(ret2, Ok(1));
            assert_eq!(&ending[..2], b"Z\0");
        }
    }
}