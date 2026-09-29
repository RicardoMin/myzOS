pub const PAGE_SIZE: usize = 4096;

pub const RAM_START: usize = 0x8000_0000;
pub const RAM_SIZE: usize = 128 * 1024 * 1024;
pub const RAM_END: usize = RAM_START + RAM_SIZE;


unsafe extern "C" {
    static __kernel_start: u8;
    static __text_end: u8;
    static __rodata_end: u8;
    static __kernel_end: u8;
}


pub struct KernelLayout {
    pub start: usize,
    pub text_end: usize,
    pub rodata_end: usize,
    pub end: usize,
}


pub fn kernel_layout() -> KernelLayout {
    KernelLayout {
        start: &raw const __kernel_start as usize,
        text_end: &raw const __text_end as usize,
        rodata_end: &raw const __rodata_end as usize,
        end: &raw const __kernel_end as usize,
    }
}


pub fn report_layout(dtb_pa: usize) {
    let layout = kernel_layout();

    assert!(PAGE_SIZE != 0, "layout: PAGE_SIZE must not be zero");

    assert!(
        RAM_START <= layout.start,
        "layout: kernel start {:#x} is below RAM start {:#x}",
        layout.start,
        RAM_START,
    );

    assert!(
        layout.start <= layout.text_end,
        "layout: start {:#x} exceeds text_end {:#x}",
        layout.start,
        layout.text_end,
    );

    assert!(
        layout.text_end <= layout.rodata_end,
        "layout: text_end {:#x} exceeds rodata_end {:#x}",
        layout.text_end,
        layout.rodata_end,
    );

    assert!(
        layout.rodata_end <= layout.end,
        "layout: rodata_end {:#x} exceeds kernel end {:#x}",
        layout.rodata_end,
        layout.end,
    );

    assert!(
        layout.start < layout.end,
        "layout: kernel range must be non-empty: [{:#x}, {:#x})",
        layout.start,
        layout.end,
    );

    assert!(
        layout.end <= RAM_END,
        "layout: kernel end {:#x} exceeds RAM end {:#x}",
        layout.end,
        RAM_END,
    );

    for (name, addr) in [
        ("start", layout.start),
        ("text_end", layout.text_end),
        ("rodata_end", layout.rodata_end),
        ("end", layout.end),
    ] {
        assert_eq!(
            addr % PAGE_SIZE,
            0,
            "layout: {}={:#x} is not aligned to PAGE_SIZE={:#x}",
            name,
            addr,
            PAGE_SIZE,
        );
    }

    let kernel_bytes = layout.end - layout.start;
    let kernel_pages = kernel_bytes / PAGE_SIZE;

    let _ = crate::console::print(format_args!(
        concat!(
            "layout: ram    [{:#x}, {:#x})\r\n",
            "layout: text   [{:#x}, {:#x})\r\n",
            "layout: rodata [{:#x}, {:#x})\r\n",
            "layout: kernel [{:#x}, {:#x})\r\n",
            "layout: dtb_pa={:#x}\r\n",
            "layout: kernel_bytes={}, kernel_pages={}\r\n",
            "layout: checks passed\r\n",
        ),
        RAM_START,
        RAM_END,
        layout.start,
        layout.text_end,
        layout.text_end,
        layout.rodata_end,
        layout.start,
        layout.end,
        dtb_pa,
        kernel_bytes,
        kernel_pages,
    ));
}