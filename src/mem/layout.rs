use super::PAGE_SIZE;

pub const USER_ENTRY: usize = 0x1000;

pub const SV39_LOW_END: usize = 1usize << 38;
pub const TRAMPOLINE: usize = SV39_LOW_END - PAGE_SIZE;
pub const USER_FRAME: usize = TRAMPOLINE - PAGE_SIZE;

pub const USER_STACK_TOP: usize = USER_FRAME;
pub const USER_STACK_BOTTOM: usize = USER_STACK_TOP - PAGE_SIZE;

pub const KSTACK_PAGES: usize = 4;
pub const KSTACK_TOP: usize = USER_STACK_BOTTOM;
pub const KSTACK_BOTTOM: usize =
    KSTACK_TOP - KSTACK_PAGES * PAGE_SIZE;
pub const KSTACK_GUARD: usize = KSTACK_BOTTOM - PAGE_SIZE;