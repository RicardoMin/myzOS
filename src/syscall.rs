use crate::{
    console,
    mem::user_vm::UserSpace,
};
use myzos_uapi::{
    E_BADARG,
    E_NOSYS,
    SYS_HELLO,
    SYS_TEST_COPYIN,
    SYS_TEST_COPYOUT,
    SYS_TEST_COPYINSTR,
};


/// # Safety
/// 用户执行已暂停；页表及相关物理页保持存活，映射不被并发修改
/// 当前内核映射允许所需读写；复制涉及的物理区域不存在冲突访问或引用
pub(crate) unsafe fn dispatch(
    number: usize,
    args: [usize; 6],
    space: &UserSpace,
) -> isize {
    match number {
        SYS_HELLO => {
            let _ = console::print(format_args!(
                "proczero: hello world!\r\n"
            ));
            
            0
        }
        SYS_TEST_COPYIN => {
            let count = args[1];
            if count > 16 {
               return E_BADARG;
            }
            let mut bytes = [0u8; 64];
            if unsafe {
                space.copy_from_user(
                    &mut bytes[..count * 4],
                    args[0],
                )
            }.is_err() {
                return E_BADARG;
            }
            
            let mut values = [0i32; 16];
            for i in 0..count {
                let base = i * 4;

                values[i] = i32::from_le_bytes([
                    bytes[base],
                    bytes[base + 1],
                    bytes[base + 2],
                    bytes[base + 3],
                ]);
            }

            let _ = console::print(format_args!(
                "copyin: {:?}\r\n",
                &values[..count],
            ));
            
            0
        }
        SYS_TEST_COPYOUT => {
            let values = [1i32, 2, 3, 4, 5];
            let mut bytes = [0u8; 20];

            for (i, value) in values.iter().enumerate() {
                let start = i * 4;
                bytes[start..start + 4].copy_from_slice(&value.to_le_bytes());
            }

            unsafe {
                if space.copy_to_user(
                    args[0], 
                    &bytes
                ).is_err() {
                    return E_BADARG;
                }
            }

            0
        }
        SYS_TEST_COPYINSTR => {
            let mut bytes = [0u8; 64];

            let len = match unsafe {
                space.copy_str_from_user(&mut bytes, args[0])
            } {
                Ok(len) => len,
                Err(_) => return E_BADARG,
            };

            let text = match core::str::from_utf8(&bytes[..len]) {
                Ok(text) => text,
                Err(_) => return E_BADARG,
            };

            let _ = console::print(format_args!(
                "copyinstr: {}\r\n",
                text,
            ));

            0
        }
        _ =>  E_NOSYS
    }
}