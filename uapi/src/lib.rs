#![no_std]

pub const SYS_HELLO: usize = 0;
pub const SYS_TEST_COPYIN: usize = 1;
pub const SYS_TEST_COPYOUT: usize = 2;
pub const SYS_TEST_COPYINSTR: usize = 3;
pub const SYS_BRK: usize = 4;

pub const E_BADARG: isize = -1;
pub const E_NOSYS: isize = -38;