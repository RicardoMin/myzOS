use crate::console;

const SYS_HELLO: usize = 0;

pub(crate) fn dispatch(number: usize) -> isize {
    match number {
        SYS_HELLO => {
            let _ = console::print(format_args!(
                "proczero: hello world!\r\n"
            ));
            
            0
        }
        _ => -38
    }
}