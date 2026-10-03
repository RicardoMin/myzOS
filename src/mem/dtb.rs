use crate::console;

use super::{RAM_END, RAM_START};


const PREFIX_SIZE: usize = 8;
const HEADER_MIN_SIZE: usize = 40;
const FDT_MAGIC: u32 = 0xd00d_feed;


pub struct DtbRange {
    pub start: usize,
    pub end: usize,
}


/// # Safety
/// dtb_pa 来自当前固件启动参数：其前 8 字节已初始化、
/// 在当前地址空间可读，且读取期间不会被其他执行者修改。
pub unsafe fn inspect(dtb_pa: usize) -> DtbRange {
    assert!(dtb_pa >= RAM_START);
    assert!(dtb_pa % 8 == 0);

    let prefix_end = dtb_pa
        .checked_add(PREFIX_SIZE)
        .expect("DTB prefix address overflow");

    assert!(
        prefix_end <= RAM_END,
        "DTB prefix exceeds RAM",
    );

    let bytes = unsafe {
        (dtb_pa as *const [u8; PREFIX_SIZE]).read()
    };

    let magic = u32::from_be_bytes([
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
    ]);

    let total_size = u32::from_be_bytes([
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
    ]) as usize;

    let _ = console::print(format_args!(
        "dtb: magic={:#x}, totalsize={}\r\n",
        magic,
        total_size
    ));

    assert!(magic == FDT_MAGIC);
    assert!(total_size >= HEADER_MIN_SIZE);

    let end = dtb_pa
        .checked_add(total_size)
        .expect("DTB end address overflow");
    assert!(end <= RAM_END);

    DtbRange {
        start: dtb_pa,
        end,
    }
}