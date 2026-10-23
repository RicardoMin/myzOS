use crate::{
    console,
    context::Context,
    mem::{
        PAGE_SIZE,
        layout::{KSTACK_TOP, USER_ENTRY},
        pmem,
        user_vm::UserSpace,
    },
};


#[derive(Debug)]
enum State {
    Created,
}


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

pub fn check() {
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
        assert_eq!(pmem::free_counts(), before);
    }

    let _ = console::print(format_args!(
        "precess: frame mapping and resource reclaim checks passed (not runnable)\r\n",
    ));
}