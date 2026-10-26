use crate::{
    console, context::{Context, arch_switch}, cpu, irq, mem::{
        PAGE_SIZE, kernel_vm, layout::{
            KSTACK_BOTTOM, KSTACK_TOP, USER_ENTRY, USER_FRAME,
        }, pmem, user_vm::UserSpace,
    }, trampoline, trap, user_frame::UserFrame,
};
use core::{
    arch::asm, mem::MaybeUninit, ptr, sync::atomic::{AtomicPtr, Ordering},
};
use crate::syscall;


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


struct Process {
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
            heap_top: USER_ENTRY + PAGE_SIZE,
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
    /// 进程从未发布或运行，且满足 UserSpace::destroy 的全部约束
    unsafe fn destroy(self) {
        unsafe { 
            self.space.destroy();
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
            "csrw sie, zero",
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

    let (sepc, sstatus, user_sp, a0, a1, a2, a7) = {
        let frame_ptr= unsafe {
            ((*current).space.frame_pa()) as *const UserFrame
        };

        let frame = unsafe { &*frame_ptr };

        (
            frame.regs.sepc,
            frame.regs.sstatus,
            frame.regs.x[2],
            frame.regs.x[10],
            frame.regs.x[11],
            frame.regs.x[12],
            frame.regs.x[17],
        )
    };

    assert_eq!(sstatus & SPP, 0);
    assert!(kernel_sp >= KSTACK_BOTTOM && kernel_sp < KSTACK_TOP);
    assert_eq!(kernel_sp % 16, 0);

    match cause {
        8 => {
            let result = syscall::dispatch(a7);

            {
                let frame = unsafe {
                    &mut *((*current).space.frame_pa() as *mut UserFrame)
                };

                frame.regs.sepc = sepc
                    .checked_add(4)
                    .expect("user return PC overflow");

                frame.regs.x[10] = result as usize;
            }

            unsafe {
                return_to_user();
            }
        }
        3 => {
            let first = a0 as isize;
            let second = a1 as isize;
            let unknown = a2 as isize;

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

            let _ = console::print(format_args!(
                "user: syscall round trip passed; halted in kernel\r\n",
            ));

            loop {
                core::hint::spin_loop();
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


pub fn check() {
    unsafe {
        crate::trampoline::check_alias();
    }

    let image = include_bytes!(env!("MYZOS_USER_IMAGE"));

    let before = pmem::free_counts();
    let process = Process::new(image);

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

    unsafe {
        process.destroy();
    }
    assert_eq!(pmem::free_counts(), before);

    let _ = console::print(format_args!(
        "process: frame mapping and resource reclaim checks passed (not runnable)\r\n",
    ));
}