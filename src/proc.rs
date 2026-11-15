use myzos_uapi::E_BADARG;

use crate::{
    console, context::{Context, arch_switch}, cpu, irq, mem::{
        PAGE_SIZE, kernel_vm, layout::{
            HEAP_START, KSTACK_BOTTOM, KSTACK_TOP, MMAP_BEGIN, USER_FRAME, USER_STACK_BOTTOM, USER_STACK_LIMIT, USER_STACK_TOP,
        }, pmem, user_vm::UserSpace,
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
            }
        }
    }

    /// # Safety
    /// 进程从未发布或运行; 独占页表及私有页，所有外部访问和借用已结束
    /// 堆顶和栈页数准确描述本进程拥有的页面
    unsafe fn destroy(mut self) {
        unsafe { 
            self.space.resize_heap(self.heap_top, HEAP_START);

            let bottom = USER_STACK_TOP - self.user_stack_pages * PAGE_SIZE;
            self.space.unmap_user_range(bottom, USER_STACK_BOTTOM);

            self.space.destroy();
        }
    }

    pub(crate) fn space(&self) -> &UserSpace {
        &self.space
    }

    /// # Safety
    /// 当前独占进程; 用户页表从未启用，或已切离并完成同步
    /// 栈页数准确，待增长区间未映射；恢复执行前须同步新增映射
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
    /// 当前独占进程，用户页表已暂停并完成同步
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

                assert_eq!(a7, 1, "user copyinstr, brk or stack checks failed");

                let _ = console::print(format_args!(
                    "user: copyinstr, brk and stack checks passed\r\n",
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

    unsafe {
        process.destroy();
    }
    assert_eq!(pmem::free_counts(), before);

    let _ = console::print(format_args!(
        "process: user mappings and resource reclaim checks passed (not runnable)\r\n",
    ));
}