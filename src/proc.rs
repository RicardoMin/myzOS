use myzos_uapi::E_BADARG;

use crate::{
    console, context::{Context, arch_switch}, cpu, irq, mem::{
        PAGE_SIZE, kernel_vm, layout::{
            HEAP_START, KSTACK_BOTTOM, KSTACK_TOP, MMAP_BEGIN, MMAP_END,
             USER_FRAME, USER_STACK_LIMIT, USER_STACK_TOP, USER_ENTRY,
        }, mmap::{self, NodeId}, pmem, user_vm::UserSpace,
    }, trampoline, trap, user_frame::UserFrame,
};
use core::{
    arch::asm,
    mem::MaybeUninit,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};
use crate::syscall;
use crate::shell;


static mut FIRST: MaybeUninit<Process> = MaybeUninit::uninit();
static CURRENT: AtomicPtr<Process> = AtomicPtr::new(ptr::null_mut());

static mut BOOT_CONTEXT: Context = Context::ZERO;


#[derive(Debug)]
enum State {
    Created,
    Running,
}


const SIE: usize = 1 << 1;
const SPIE: usize = 1 << 5;
const SPP: usize= 1 << 8;
const VS: usize = 1 << 9 | 1 << 10;
const FS: usize = 1 << 13 | 1 << 14;
const SUM: usize = 1 << 18;
const MXR: usize = 1 << 19;


pub(crate) struct Process {
    pid: usize,
    state: State,
    space: UserSpace,
    heap_top: usize,
    user_stack_pages: usize,
    kstack_top: usize,
    context: Context,
    mmap_head: Option<NodeId>,
}


impl Process {
    fn new(image: &[u8]) -> Self {
        Self {
            pid: 0,
            state: State::Created,
            space: UserSpace::new(image),
            heap_top: HEAP_START,
            user_stack_pages: 1,
            kstack_top: KSTACK_TOP,
            context: Context {
                ra: 0,
                sp: KSTACK_TOP,
                saved: [0; 12],
            },
            mmap_head: None,
        }
    }

    /// # Safety
    /// 进程从未发布或运行; 独占页表及私有页，所有外部访问和借用已结束
    /// 堆顶、栈页数和 mmap 链准确描述本进程拥有的私有页面及区域节点
    unsafe fn destroy(mut self) {
        unsafe { 
            while let Some(id) = self.mmap_head  {
                let mut pool = mmap::POOL.lock();
                self.mmap_head = pool.get(id).next;
                pool.free(id);
            }

            self.space.destroy();
        }
    }

    pub(crate) fn space(&self) -> &UserSpace {
        &self.space
    }

    /// # Safety
    /// 当前独占进程; 用户页表从未启用，或已切离并完成同步
    /// 堆顶准确描述私有堆页；收缩区无存活引用或访问，恢复执行前须同步页表修改
    pub(crate) unsafe fn brk(&mut self, new_top: usize) -> isize {
        if new_top == 0 {
            return self.heap_top as isize;
        }

        if new_top % PAGE_SIZE != 0 {
            return E_BADARG;
        }

        if new_top < HEAP_START || new_top > MMAP_BEGIN {
            return E_BADARG;
        }

        unsafe {
            self.space.resize_heap(self.heap_top, new_top);
        }

        self.heap_top = new_top;

        new_top as isize
    }

    /// # Safety
    /// 当前独占进程，用户页表从未启用，或已切离并完成同步
    /// 栈页数准确，待增长区间未映射；恢复执行前须同步新增映射
    unsafe fn grow_stack(&mut self, fault_va: usize) -> bool {
        let old_bottom =
            USER_STACK_TOP - self.user_stack_pages * PAGE_SIZE;

        if fault_va < USER_STACK_LIMIT {
            return false;
        }

        if fault_va >= old_bottom {
            return false;
        }

        let new_bottom = fault_va & !(PAGE_SIZE - 1);

        unsafe {
            self.space.map_user_range(new_bottom, old_bottom);
        }

        self.user_stack_pages = (
            USER_STACK_TOP - new_bottom
        ) / PAGE_SIZE;

        true
    }

    /// # Safety
    /// 当前独占进程及区域链；用户页表从未启用，或已切离并完成同步
    /// 区域链准确描述已有私有映射，恢复执行前须同步新增映射
    pub(crate) unsafe fn mmap(
        &mut self,
        begin: usize,
        len: usize,
    ) -> isize {
        let actual = {
            let mut pool = mmap::POOL.lock();
            if let Some(begin) = pool.reserve_region(&mut self.mmap_head, begin, len) {
                begin
            } else {
                return E_BADARG;
            }
        };

        unsafe {
            self.space.map_user_range(actual, actual + len);
        }

        let _ = console::print(format_args!(
            "mmap: mapped [{:#x}, {:#x})\r\n",
            actual,
            actual + len,
        ));

        actual as isize
    }

    /// # Safety
    /// 当前独占进程及区域链，用户页表从未启用或已切离并完成同步
    /// 待回收页面没有存活引用或并发访问，恢复用户执行前须同步页表修改
    pub(crate) unsafe fn munmap(
        &mut self,
        begin: usize,
        len: usize
    ) -> isize {
        let Some(end) = begin.checked_add(len) else {
            return E_BADARG;
        };
        if len == 0 {
            return E_BADARG;
        }
        if begin % PAGE_SIZE != 0 || len % PAGE_SIZE != 0 {
            return E_BADARG;
        }
        if begin < MMAP_BEGIN || end > MMAP_END {
            return E_BADARG;
        }

        let mut prev = None;

        loop {
            let cut = {
                let mut pool = mmap::POOL.lock();
                pool.cut_next(&mut self.mmap_head, &mut prev, begin, end)
            };

            let Some((lo, hi)) = cut else { break };

            unsafe {
                self.space.unmap_user_range(lo, hi);
            }
        }

        let _ = console::print(format_args!(
            "munmap: request [{:#x}, {:#x}) completed\r\n",
            begin,
            end,
        ));

        0
    }

    /// # Safety
    /// 复制期间，源进程的区域链、页表和数据页保持稳定，无并发写入或回收。
    /// dst_root 指向独立、独占且未启用的有效页表，待复制区间尚未映射。
    unsafe fn copy_user_pages_to(&self, dst_root: usize) {
        unsafe {
            // 用户镜像
            self.space.copy_range_to(dst_root, USER_ENTRY, USER_ENTRY + PAGE_SIZE);
            // 堆
            self.space.copy_range_to(dst_root, HEAP_START, self.heap_top);
            // 栈
            self.space.copy_range_to(
                dst_root,
                USER_STACK_TOP - self.user_stack_pages * PAGE_SIZE,
                USER_STACK_TOP,
            );
            // mmap 记录
            let mut cur = self.mmap_head;
            while let Some(id) = cur {
                let region = {
                    let pool = mmap::POOL.lock();
                    *pool.get(id)
                };
                self.space.copy_range_to(
                    dst_root, region.begin, region.begin + region.pages * PAGE_SIZE
                );
                cur = region.next;
            }
        }
    }
}


/// # Safety
/// 当前 hart 独占当前进程及现场，内核页表有效
/// 当前关中断、IRQ 深度为零，无持锁和跨越交接的现场引用
unsafe fn return_to_user() -> ! {
    assert!(!irq::enabled());
    assert_eq!(irq::depth(), 0);

    let current = CURRENT.load(Ordering::Relaxed);
    assert!(!current.is_null());

    let (user_satp, frame_pa) = unsafe {
        let process = &*current;

        (
            process.space.satp(),
            process.space.frame_pa(),
        )
    };

    let status: usize;

    unsafe {
        asm!(
            "csrr {status}, sstatus",
            status = out(reg) status,
            options(nostack),
        );
    }

    {
        let frame = unsafe {
            &mut *(frame_pa as *mut UserFrame)
        };

        frame.regs.sstatus =
            (status & !(SIE | SPP | VS | FS | SUM | MXR)) | SPIE;
    }

    let trap_entry = trampoline::user_trap_entry();

    let enter = unsafe {
        core::mem::transmute::<
            usize,
            unsafe extern "C" fn(usize, usize) -> !
        >(trampoline::user_return_entry())
    };

    unsafe {
        asm!(
            "csrw stvec, {entry}",
            "fence.i",
            entry = in(reg) trap_entry,
            options(nostack),
        );

        enter(USER_FRAME, user_satp)
    }
}

extern "C" fn first_entry() -> ! {
    assert!(!irq::enabled());
    assert_eq!(irq::depth(), 0);

    let sp: usize;

    unsafe {
        asm!(
            "mv {sp}, sp",
            sp = out(reg) sp,
            options(nostack),
        );
    }

    assert!(KSTACK_BOTTOM <= sp && sp <= KSTACK_TOP);
    assert_eq!(sp % 16, 0);

    let current = CURRENT.load(Ordering::Relaxed);
    assert!(!current.is_null());

    let (pid, expected_hart) = unsafe {
        let process = &*current;
        let frame = &*(process.space.frame_pa() as *const UserFrame);

        (process.pid, frame.kernel_hart)
    };

    let hart = cpu::hart_id();
    assert_eq!(hart, expected_hart);

    let _ = console::print(format_args!(
        "process: pid={} entered kernel context on hart={}\r\n",
        pid,
        hart,
    ));

    unsafe {
        return_to_user();
    }
}


/// # Safety
/// 仅启动 hart 调用一次；当前关中断、无持锁且 IRQ 深度为零
/// 内核页表已启用，首进程内核栈有效且本次执行独占
pub unsafe fn enter_first() -> ! {
    assert!(!irq::enabled());
    assert_eq!(irq::depth(), 0);
    assert!(CURRENT.load(Ordering::Relaxed).is_null());

    let image = include_bytes!(env!("MYZOS_USER_IMAGE"));
    let slot = (&raw mut FIRST).cast::<Process>();

    unsafe {
        slot.write(Process::new(image));
    }

    let kernel_gp: usize;

    unsafe {
        asm!(
            "mv {kernel_gp}, gp",
            kernel_gp = out(reg) kernel_gp,
            options(nostack),
        );
    }

    {
        let process = unsafe { &mut *slot };

        let frame = unsafe {
            &mut *(process.space.frame_pa() as *mut UserFrame)
        };

        frame.kernel_satp = kernel_vm::read_satp();
        frame.kernel_sp = process.kstack_top;
        frame.kernel_entry = user_trap as *const () as usize;
        frame.kernel_hart = cpu::hart_id();
        frame.kernel_gp = kernel_gp;

        process.context = Context {
            ra: first_entry as *const () as usize,
            sp: process.kstack_top,
            saved: [0; 12],
        };

        process.state = State::Running;
    }

    CURRENT.store(slot, Ordering::Relaxed);

    unsafe {
        arch_switch(
            &raw mut BOOT_CONTEXT,
            &raw const (*slot).context,
        );
    }

    panic!("process: boot context resumed unexpectedly");
}

extern "C" fn user_trap() -> ! {
    trap::init_hart();

    let cause: usize;
    let value: usize;
    let kernel_sp: usize;

    unsafe {
        asm!(
            "csrr {cause}, scause",
            "csrr {value}, stval",
            "mv {kernel_sp}, sp",
            cause = out(reg) cause,
            value = out(reg) value,
            kernel_sp = out(reg) kernel_sp,
            options(nostack),
        );
    }

    let current = CURRENT.load(Ordering::Relaxed);
    assert!(!current.is_null());

    let (sepc, sstatus, user_sp, args, a7) = {
        let frame_ptr= unsafe {
            ((*current).space.frame_pa()) as *const UserFrame
        };

        let frame = unsafe { &*frame_ptr };

        let args = [
            frame.regs.x[10],
            frame.regs.x[11],
            frame.regs.x[12],
            frame.regs.x[13],
            frame.regs.x[14],
            frame.regs.x[15],
        ];

        (
            frame.regs.sepc,
            frame.regs.sstatus,
            frame.regs.x[2],
            args,
            frame.regs.x[17],
        )
    };

    assert_eq!(sstatus & SPP, 0);
    assert!(kernel_sp >= KSTACK_BOTTOM && kernel_sp < KSTACK_TOP);
    assert_eq!(kernel_sp % 16, 0);

    if !trap::handle_interrupt(cause) {
        match cause {
            8 => {
                let result = unsafe {
                    let process = &mut *current;
                    syscall::dispatch(a7, args, process)
                };

                {
                    let frame = unsafe {
                        &mut *((*current).space.frame_pa() as *mut UserFrame)
                    };

                    frame.regs.sepc = sepc
                        .checked_add(4)
                        .expect("user return PC overflow");

                    frame.regs.x[10] = result as usize;
                }
            }
            3 => {
                let first = args[0] as isize;
                let second = args[1] as isize;
                let unknown = args[2] as isize;

                assert_eq!(
                    (
                        args[3] as isize,
                        args[4] as isize,
                        args[5] as isize,
                    ),
                    (0, E_BADARG, E_BADARG),
                    "user copyin results are incorrect",
                );

                let _ = console::print(format_args!(
                    "user: copyin checks passed\r\n",
                ));

                let _ = console::print(format_args!(
                    "user: syscall returns: first={}, second={}, unknown={}\r\n",
                    first,
                    second,
                    unknown,
                ));

                assert_eq!(
                    (first, second, unknown),
                    (0, 0, -38),
                    "user syscall return values are incorrect",
                );

                let copyout_ok = unsafe {
                    let frame = &*((*current).space.frame_pa() as *const UserFrame);
                    frame.regs.x[16]
                };

                assert_eq!(copyout_ok, 1, "user copyout check failed");

                assert_eq!(a7, 1, "user copyinstr, brk, stack, mmap and munmap checks failed");

                let _ = console::print(format_args!(
                    "user: copyinstr, brk, stack, mmap and munmap checks passed\r\n",
                ));

                let _ = console::print(format_args!(
                    concat!(
                        "user: syscall round trip passed; returning to user mode\r\n",
                        "myzOS console ready while user code runs. Type help and press Enter.\r\n",
                    )
                ));

                let _ = console::print(format_args!(
                    "user: copyout check passed\r\n",
                ));

                {
                    let frame = unsafe {
                        &mut *((*current).space.frame_pa() as *mut UserFrame)
                    };

                    frame.regs.sepc = sepc
                        .checked_add(4)
                        .expect("user test marker PC overflow");
                }
            }
            13 | 15 => {
                let (ret, new_pages) = unsafe {
                    let process = &mut *current;

                    let ans = process.grow_stack(value);

                    (ans, process.user_stack_pages)
                };

                if !ret {
                    trap::rust_trap_fatal(
                        cause,
                        sepc,
                        value,
                        sstatus,
                        user_sp,
                    );
                } else {
                    let _ = console::print(format_args!(
                        "stack: fault={:#x}, pages={}\r\n",
                        value,
                        new_pages,
                    ));
                }
            }
            _ => {
                trap::rust_trap_fatal(
                    cause,
                    sepc,
                    value,
                    sstatus,
                    user_sp,
                );
            }
        }
    }

    shell::poll_input();

    unsafe {
        return_to_user();
    }
}


pub fn check() {
    unsafe {
        crate::trampoline::check_alias();
    }

    let image = include_bytes!(env!("MYZOS_USER_IMAGE"));

    let before = pmem::free_counts();
    let initial_nodes = mmap::POOL.lock().free_count();
    let mut process = Process::new(image);

    unsafe {process.space.check_layout(image)};

    let _ = console::print(format_args!(
        concat!(
            "process: pid={} state={:?} heap_top={:#x} ",
            "user_stack_pages={} kstack_top={:#x} ",
            "context_ra={:#x} context_sp={:#x}\r\n",
        ),
        process.pid,
        process.state,
        process.heap_top,
        process.user_stack_pages,
        process.kstack_top,
        process.context.ra,
        process.context.sp,
    ));

    let ret = unsafe {
        process.brk(HEAP_START + PAGE_SIZE)
    };
    assert_eq!(ret as usize, HEAP_START + PAGE_SIZE);

    unsafe {
        assert_eq!(process.grow_stack(USER_STACK_LIMIT - 1), false);
        assert_eq!(process.grow_stack(USER_STACK_TOP - 1), false);
        assert_eq!(process.grow_stack(USER_STACK_TOP - 3 * PAGE_SIZE), true);
        assert_eq!(process.user_stack_pages, 3);
    }

    let b = MMAP_BEGIN;
    let p = PAGE_SIZE;
    unsafe {
        assert_eq!(process.mmap(b + 4 * p, 3 * p), (b + 4 * p) as isize);
        assert_eq!(process.mmap(b + 10 * p, 2 * p), (b + 10 * p) as isize);
        assert_eq!(process.mmap(b + 2 * p, 2 * p), (b + 2 * p) as isize);
        assert_eq!(process.mmap(b + 12 * p, p), (b + 12 * p) as isize);
        assert_eq!(process.mmap(0, 2 * p), b as isize);
        assert_eq!(process.mmap(0, 3 * p), (b + 7 * p) as isize);
        assert_eq!(process.mmap(0, 10 * p), (b + 13 * p) as isize);
        if let Some(id) = process.mmap_head {
            let pool = mmap::POOL.lock();
            let region = pool.get(id);
            assert_eq!(
                (region.begin, region.pages, region.next),
                (b, 23, None),
            );
            assert_eq!(pool.free_count(), initial_nodes - 1);
        }
        assert_eq!(process.mmap(b + p, 2 * p), E_BADARG);
        assert_eq!(process.mmap(b + 24 * p + 1, p), E_BADARG);

        assert_eq!(process.munmap(b + 8 * p, 3 * p), 0);
        if let Some(id) = process.mmap_head {
            let pool = mmap::POOL.lock();
            let region = pool.get(id);
            assert_eq!(
                (region.begin, region.pages),
                (b, 8),
            );
            let r_id = region.next.expect("expected right region after split");
            let right = pool.get(r_id);
            assert_eq!(
                (right.begin, right.pages),
                (b + 11 * p, 12),
            );
        }
    }

    unsafe {
        process.destroy();
    }
    assert_eq!(pmem::free_counts(), before);
    assert_eq!(mmap::POOL.lock().free_count(), initial_nodes);

    let _ = console::print(format_args!(
        concat!(
            "process: user mappings and resource reclaim checks passed (not runnable)\r\n",
            "mmap: first-fit, merge and reclaim checks passed\r\n",
        ),
    ));
}