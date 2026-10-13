//! Lab2 自检：编码、页表生命周期、页池耗尽与双 hart 分配。
//! 启动前自检要求单 hart、页池无其他使用者；双 hart 入口每次启动各调用一次。

use core::sync::atomic::{AtomicUsize, Ordering};

use super::{
    PAGE_SIZE, RAM_END, RAM_START,
    pagetable::{self, MapError, UnmapError},
    pmem,
    vm::{self, PTE_R, PTE_W, PTE_X, Pte},
};
use crate::{console, cpu, spinlock::SpinLock};

const ROUNDS: usize = 128;
static HELD: SpinLock<[[usize; 2]; 2]> = SpinLock::new([[0; 2]; 2]);
static ROUND_START: AtomicUsize = AtomicUsize::new(0);
static SECONDARY_HOLDING: AtomicUsize = AtomicUsize::new(0);
static RELEASE_ROUND: AtomicUsize = AtomicUsize::new(0);
static SECONDARY_DONE: AtomicUsize = AtomicUsize::new(0);

/// # Safety
/// page 是存活、已初始化、可读的一整页，检查期间没有并发写入。
unsafe fn assert_page_filled(page: usize, expected: u8) {
    for offset in 0..PAGE_SIZE {
        assert_eq!(
            unsafe { (page as *const u8).add(offset).read() },
            expected
        );
    }
}

/// 验证最后一页可分配、耗尽返回 None、归还后能清零复用。
pub fn check_exhaustion(pool: &mut pmem::PagePool) {
    pool.check_initial_chain();
    let before = pool.free_pages();
    let mut held = 0usize;
    let mut allocated = 0;

    while let Some(pa) = pool.try_alloc() {
        assert!(allocated < before);
        // SAFETY: 页已从 pool 取出，由测试独占；借页首保存测试持有链。
        unsafe {
            (pa as *mut usize).write(held);
        }
        held = pa;
        allocated += 1;
    }
    assert_eq!(allocated, before);
    assert_eq!(pool.free_pages(), 0);
    assert_eq!(pool.try_alloc(), None);

    // 先完整只读审计，避免发现链有环之前已经重复归还页面。
    let mut cursor = held;
    let mut seen = 0;
    while cursor != 0 {
        assert!(seen < allocated);
        assert!(RAM_START <= cursor && cursor < RAM_END);
        assert_eq!(cursor % PAGE_SIZE, 0);
        cursor = unsafe { (cursor as *const usize).read() };
        seen += 1;
    }
    assert_eq!(seen, allocated);

    cursor = held;
    while cursor != 0 {
        // SAFETY: 持有链已审计；先读 next，归还后不再访问该页。
        let next = unsafe { (cursor as *const usize).read() };
        unsafe {
            pool.free(cursor);
        }
        cursor = next;
    }
    pool.check_initial_chain();

    let page = pool.alloc();
    // SAFETY: 新申请的页仍由测试独占，检查后只归还一次。
    unsafe {
        assert_page_filled(page, 0);
        pool.free(page);
    }
    pool.check_initial_chain();
    let _ = console::print(format_args!(
        "pmem: exhausted, restored and reused {} pages\r\n",
        before,
    ));
}

/// 固定期望值避免编码器与解码器错误一致时互相掩盖。
pub fn check_vm_encoding() {
    assert_eq!(
        core::mem::size_of::<Pte>() * pagetable::ENTRIES,
        PAGE_SIZE
    );
    for (va, indices, offset) in [
        (0x4040_3abc, [1, 2, 3], 0xabc),
        (
            0x0000_003f_ffff_ffff,
            [255, 511, 511],
            0xfff,
        ),
        (0xffff_ffc0_0000_0000, [256, 0, 0], 0),
    ] {
        assert_eq!(vm::vpn_indices(va), Some(indices));
        assert_eq!(vm::page_offset(va), offset);
    }
    assert_eq!(
        vm::vpn_indices(0x0000_0040_0000_0000),
        None
    );

    let pa = 0x8020_1000;
    let table = Pte::table(pa).unwrap();
    assert_eq!(table.bits(), 0x2008_0401);
    assert!(!table.is_leaf());
    assert!(!Pte::empty().is_valid());

    for (permissions, bits) in [(PTE_R | PTE_X, 0x2008_044b), (PTE_R | PTE_W, 0x2008_04c7)] {
        let leaf = Pte::leaf(pa, permissions).unwrap();
        assert_eq!(leaf.bits(), bits);
        assert_eq!(leaf.pa(), pa);
        assert!(leaf.is_leaf());
    }
    let high_pa = 0x00ff_ffff_ffff_f000;
    assert_eq!(
        Pte::table(high_pa).unwrap().pa(),
        high_pa
    );
    assert!(Pte::table(1usize << 56).is_none());
    assert!(Pte::leaf(pa + 1, PTE_R).is_none());
    assert!(Pte::leaf(pa, PTE_W).is_none());
    assert!(Pte::leaf(pa, PTE_R | (1 << 4)).is_none());

    let _ = console::print(format_args!(
        "sv39: encoding checks passed\r\n"
    ));
}

/// 同一棵树覆盖共享路径、数据别名、删除、空表回收与重新映射。
pub fn check_page_tables() {
    let before = pmem::free_counts();
    let root = pmem::alloc_kernel();
    let data = [pmem::alloc_data(), pmem::alloc_data()];
    let mappings = [
        (0x4040_3000, data[0], PTE_R | PTE_W),
        (0x4040_4000, data[0], PTE_R),
        (0x4060_3000, data[1], PTE_R | PTE_W),
    ];
    let allocated = pmem::free_counts();

    // SAFETY: 页表和数据均由本测试持有，无并发访问，页表从未交给硬件。
    unsafe {
        // 只留两个代表性的输入错误，确认失败不会提前分配子表。
        assert_eq!(
            pagetable::map_4k(root, mappings[0].0 + 1, data[0], PTE_R),
            Err(MapError::InvalidVa),
        );
        assert_eq!(
            pagetable::map_4k(root, mappings[0].0, data[0], PTE_W),
            Err(MapError::InvalidLeaf),
        );
        assert_eq!(pmem::free_counts(), allocated);
        assert_eq!(pagetable::read_pte(root, 1).bits(), 0);

        for (i, pa) in data.iter().copied().enumerate() {
            (pa as *mut u8).add(0xabc).write(0x51 + i as u8);
        }
        for (i, &(va, pa, permissions)) in mappings.iter().enumerate() {
            assert_eq!(
                pagetable::map_4k(root, va, pa, permissions),
                Ok(())
            );
            assert_eq!(
                pmem::free_counts(),
                (before.0 - [3, 3, 4][i], before.1 - 2),
            );
            assert_eq!(
                pagetable::translate_4k(root, va + 0xabc),
                Some(pa + 0xabc)
            );
        }

        // 重复映射必须拒绝覆盖；地址和权限分别由这里及 unmap 返回值验证。
        let held = pmem::free_counts();
        assert_eq!(
            pagetable::map_4k(root, mappings[0].0, data[1], PTE_R),
            Err(MapError::SlotOccupied),
        );
        assert_eq!(
            pagetable::translate_4k(root, mappings[0].0),
            Some(data[0])
        );
        assert_eq!(
            pagetable::unmap_4k(root, mappings[0].0 + 1).err(),
            Some(UnmapError::InvalidVa),
        );
        assert_eq!(pmem::free_counts(), held);

        for (i, &(va, pa, permissions)) in mappings.iter().enumerate() {
            let old = pagetable::unmap_4k(root, va).expect("mapping missing");
            assert_eq!(
                old.bits(),
                Pte::leaf(pa, permissions).unwrap().bits()
            );
            let remaining = (before.0 - [4, 3, 1][i], before.1 - 2);
            assert_eq!(pmem::free_counts(), remaining);

            // A/B 共享物理页：删除 A 不能破坏 B，更不能释放其数据。
            for (j, &(other_va, other_pa, _)) in mappings.iter().enumerate() {
                let expected = if j <= i { None } else { Some(other_pa) };
                assert_eq!(
                    pagetable::translate_4k(root, other_va),
                    expected
                );
            }
            for (j, pa) in data.iter().copied().enumerate() {
                assert_eq!(
                    (pa as *const u8).add(0xabc).read(),
                    0x51 + j as u8
                );
            }
            // 随生命周期自然覆盖末级、中层、根路径缺失，不另建负例树。
            assert_eq!(
                pagetable::unmap_4k(root, mappings[0].0).err(),
                Some(UnmapError::NotMapped),
            );
            assert_eq!(pmem::free_counts(), remaining);
        }

        // 空树还能重建；不要求新子表地址与旧地址不同。
        let (va, pa, _) = mappings[0];
        let permissions = PTE_R;
        pagetable::map_4k(root, va, pa, permissions).unwrap();
        assert_eq!(
            pmem::free_counts(),
            (before.0 - 3, before.1 - 2)
        );
        assert_eq!(
            pagetable::translate_4k(root, va),
            Some(pa)
        );
        assert_eq!(
            pagetable::unmap_4k(root, va).unwrap().bits(),
            Pte::leaf(pa, permissions).unwrap().bits(),
        );
        assert_eq!(
            pmem::free_counts(),
            (before.0 - 1, before.1 - 2)
        );

        // 子表已由 unmap 回收；数据页按实际所有权归还，不按映射条数归还。
        for pa in data {
            pmem::free_data(pa);
        }
        pmem::free_kernel(root);
    }
    assert_eq!(pmem::free_counts(), before);
    let _ = console::print(format_args!(
        "sv39: mapping lifecycle checks passed (paging on)\r\n"
    ));
}

fn allocate_and_record(hart_id: usize) -> [usize; 2] {
    let pages = [pmem::alloc_kernel(), pmem::alloc_data()];
    HELD.lock()[hart_id] = pages;
    pages
}

/// # Safety
/// 两页分别来自 kernel/data 池，由当前 hart 独占，未归还且互不重叠。
unsafe fn check_and_return(pages: [usize; 2], pattern: u8) {
    for pa in pages {
        unsafe {
            assert_page_filled(pa, 0);
            core::ptr::write_bytes(pa as *mut u8, pattern, PAGE_SIZE);
            assert_page_filled(pa, pattern);
        }
    }
    unsafe {
        pmem::free_kernel(pages[0]);
        pmem::free_data(pages[1]);
    }
}

fn wait_for_round(progress: &AtomicUsize, round: usize) {
    while progress.load(Ordering::Acquire) < round {
        core::hint::spin_loop();
    }
}

pub fn run_primary() {
    let before = pmem::free_counts();
    for round in 1..=ROUNDS {
        ROUND_START.store(round, Ordering::Release);
        let pages = allocate_and_record(cpu::hart_id());
        wait_for_round(&SECONDARY_HOLDING, round);

        let held = *HELD.lock();
        let addresses = [held[0][0], held[0][1], held[1][0], held[1][1]];
        for i in 0..addresses.len() {
            for j in i + 1..addresses.len() {
                assert_ne!(
                    addresses[i], addresses[j],
                    "duplicate page in round {}",
                    round
                );
            }
        }
        assert_eq!(
            pmem::free_counts(),
            (before.0 - 2, before.1 - 2)
        );

        // 两核都持有页面且判重完成，才放行读写和归还。
        RELEASE_ROUND.store(round, Ordering::Release);
        unsafe {
            check_and_return(pages, round as u8);
        }
        wait_for_round(&SECONDARY_DONE, round);
        assert_eq!(
            pmem::free_counts(),
            before,
            "page leak in round {}",
            round
        );
    }
    pmem::check_all_free();
    let _ = console::print(format_args!(
        "pmem: {} two-hart rounds passed\r\n",
        ROUNDS
    ));
}

pub fn run_secondary() {
    for round in 1..=ROUNDS {
        wait_for_round(&ROUND_START, round);
        let pages = allocate_and_record(cpu::hart_id());
        SECONDARY_HOLDING.store(round, Ordering::Release);
        wait_for_round(&RELEASE_ROUND, round);
        unsafe {
            check_and_return(pages, !(round as u8));
        }
        SECONDARY_DONE.store(round, Ordering::Release);
    }
}


pub unsafe fn check_kernel_mapping(root_pa: usize) {
    let layout = super::kernel_layout();

    let cases = [
    (layout.start, PTE_R | PTE_X),
    (layout.text_end, PTE_R),
    (layout.rodata_end, PTE_R | PTE_W),
    (root_pa, PTE_R | PTE_W),
    (RAM_END - 1, PTE_R | PTE_W),
    (crate::uart::UART_BASE, PTE_R | PTE_W),
    ];

    for (va, permissions) in cases {
        let pte = unsafe {pagetable::lookup_4k(root_pa, va)}
            .expect("kernel mapping missing");

        assert_eq!(pte.pa() + vm::page_offset(va), va);
        assert_eq!(pte.flags() & (PTE_R | PTE_W | PTE_X), permissions);
    }

    assert!(
        unsafe { pagetable::lookup_4k(root_pa, RAM_START)}.is_none()
    );

    let _ = console::print(format_args!(
        "kernel_vm: layout and permissions checked (paging off)\r\n",
    ));
}