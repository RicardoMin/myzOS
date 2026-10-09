use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{console, cpu, mem::{RAM_END, RAM_START, pagetable::ENTRIES, pmem::free_counts}, spinlock::SpinLock};
use super::{pmem, PAGE_SIZE};

static HELD: SpinLock<[[usize; 2]; 2]> = 
    SpinLock::new([[0; 2]; 2]);

const ROUNDS: usize = 128;

static ROUND_START: AtomicUsize = AtomicUsize::new(0);
static SECONDARY_HOLDING: AtomicUsize = AtomicUsize::new(0);
static RELEASE_ROUND: AtomicUsize = AtomicUsize::new(0);
static SECONDARY_DONE: AtomicUsize = AtomicUsize::new(0);


fn allocate_and_record(hart_id: usize) -> [usize; 2] {
    let kernel_pa = pmem::alloc_kernel();
    let data_pa = pmem::alloc_data();
    let pages = [kernel_pa, data_pa];

    {
        let mut held = HELD.lock();
        held[hart_id] = pages;
    }

    pages
}

/// # Safety
/// page 必须指向一整页有效、可读且已初始化的内存；
/// 检查期间不能有其他执行者修改该页
unsafe fn assert_page_filled(page: usize, expected: u8) {
    for offset in 0..PAGE_SIZE {
        let actual = unsafe {
            (page as *const u8).add(offset).read()
        };
        assert_eq!(actual, expected);
    }
}

/// # Safety
/// pages[0]、pages[1] 分别是当前独占的内核池、数据池页面；
/// 两页互不重叠、尚未归还，且没有其他访问者
unsafe fn check_and_return(pages: [usize; 2], pattern: u8) {
    for pa in pages {
        unsafe {
            assert_page_filled(pa, 0);

            core::ptr::write_bytes(
                pa as *mut u8,
                pattern,
                PAGE_SIZE,
            );

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
    let before = free_counts();

    let _ = console::print(format_args!(
        "pmem: starting {} two-hart rounds\r\n",
        ROUNDS,
    ));

    for round in 1..=ROUNDS {

        ROUND_START.store(round, Ordering::Release);

        let pages = allocate_and_record(cpu::hart_id());

        wait_for_round(&SECONDARY_HOLDING, round);

        let held = {
            let guard = HELD.lock();
            *guard
        };

        let addresses = [
            held[0][0],
            held[0][1],
            held[1][0],
            held[1][1],
        ];
        if round == 1 || round == ROUNDS {
            let _ = console::print(format_args!(
                concat!(
                    "hart0: kernel_page:[{}], data_page:[{}]\r\n",
                    "hart1: kernel_page:[{}], data_page:[{}]\r\n"
                ),
                addresses[0],
                addresses[1],
                addresses[2],
                addresses[3],
            ));
        }
        for i in 0..4 {
            for j in i+1..4 {
                assert_ne!(addresses[i], addresses[j]);
            }
            assert_ne!(addresses[i], 0);
            assert_eq!(addresses[i] % PAGE_SIZE, 0);
        }
        assert_eq!(free_counts(), (before.0 -2, before.1 - 2));

        RELEASE_ROUND.store(round, Ordering::Release);

        unsafe {
            check_and_return(pages, round as u8);
        }

        wait_for_round(&SECONDARY_DONE, round);

        assert_eq!(
            free_counts(),
            before,
            "round {}: free counts did not recover",
            round,
        );
    }

    pmem::check_all_free();

    assert_eq!(
        SECONDARY_DONE.load(Ordering::Acquire),
        ROUNDS,
    );

    let _ = console::print(format_args!(
        "pmem: {} two-hart rounds passed\r\n",
        ROUNDS,
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

pub fn check_exhaustion(pool: &mut pmem::PagePool) {
    let before = pool.free_pages();
    let mut held = 0usize;
    let mut allocated = 0usize;

    pool.check_initial_chain();

    while let Some(pa) = pool.try_alloc() {
        assert!(allocated < before);

        unsafe {
            (pa as *mut usize).write(held);
        }
        held = pa;
        allocated += 1;

        assert_eq!(pool.free_pages(), before - allocated);
    }

    assert_eq!(allocated, before);
    assert_eq!(pool.free_pages(), 0);
    assert_eq!(pool.try_alloc(), None);

    pool.check_free_chain();

    let mut cursor = held;
    let mut seen = 0;

    while cursor != 0 {
        assert!(seen < allocated);
        assert!(RAM_START <= cursor && cursor < RAM_END);
        assert_eq!(cursor % PAGE_SIZE, 0);

        let next = unsafe {
            (cursor as *const usize).read()
        };
        cursor = next;
        seen += 1;
    }

    assert_eq!(seen, allocated);

    cursor = held;

    while cursor != 0 {
        let next = unsafe {
            (cursor as *const usize).read()
        };

        unsafe {
            pool.free(cursor);
        }

        cursor = next;
    }

    assert_eq!(pool.free_pages(), before);
    pool.check_initial_chain();

    let page = pool
        .try_alloc()
        .expect("pool did not recover");
    unsafe {
        assert_page_filled(page, 0);
        pool.free(page);
    }
    assert_eq!(pool.free_pages(), before);
    pool.check_initial_chain();

    let _ = console::print(format_args!(
            "pmem: exhausted and restored {} pages\r\n",
            before,
        ),
    );
}


pub fn check_sv39_addresses() {
    use super::vm;

    assert_eq!(usize::BITS, 64);
    assert_eq!(PAGE_SIZE, 4096);

    let cases = [
        (0usize, [0, 0, 0], 0),
        (0x0000_0000_4040_3abc, [1, 2, 3], 0xabc),
        (0x0000_0000_8020_1234, [2, 1, 1], 0x234),
        (0x0000_003f_ffff_ffff, [255, 511, 511], 0xfff),
        (0xffff_ffc0_0000_0000, [256, 0, 0], 0),
        (0xffff_ffff_ffff_ffff, [511, 511, 511], 0xfff),
    ];

    for (va, expected_indices, expected_offset) in cases {
        assert!(vm::is_canonical(va), "va={:#x}", va);

        assert_eq!(
            vm::vpn_indices(va),
            Some(expected_indices),
            "va={:#x}",
            va,
        );

        assert_eq!(
            vm::page_offset(va),
            expected_offset,
            "va={:#x}",
            va,
        );
    }

    for va in [
        0x0000_0040_0000_0000usize,
        0xffff_ffbf_ffff_ffff,
        0x8000_0000_0000_0000,
    ] {
        assert!(!vm::is_canonical(va), "va={:#x}", va);
        assert_eq!(vm::vpn_indices(va), None, "va={:#x}", va);
    }

    let _ = console::print(format_args!(
        "sv39: address checks passed\r\n",
    ));
}


pub fn check_ptes() {
    use super::vm::{Pte, PTE_R, PTE_W, PTE_X, PTE_V, PTE_A};

    assert_eq!(core::mem::size_of::<Pte>(), 8);

    let empty = Pte::empty();
    assert_eq!(empty.bits(), 0);
    assert!(!empty.is_valid());
    assert!(!empty.is_leaf());

    let pa = 0x8020_1000;

    let mut table = Pte::table(pa).expect("table encoding failed");
    assert_eq!(table.bits(), 0x2008_0401);
    assert_eq!(table.pa(), pa);
    assert_eq!(table.flags(), 1);
    assert!(table.is_valid());
    assert!(!table.is_leaf());

    for (permissions, expected_bits, expected_flags) in [
        (PTE_R, 0x2008_0443, 0x43),
        (PTE_X, 0x2008_0449, 0x49),
        (PTE_R | PTE_X, 0x2008_044b, 0x4b),
        (PTE_R | PTE_W, 0x2008_04c7, 0xc7),
        (PTE_R | PTE_W | PTE_X, 0x2008_04cf, 0xcf),
    ] {
        let pte = Pte::leaf(pa, permissions)
            .expect("leaf encoding failed");

        assert_eq!(pte.bits(), expected_bits);
        assert_eq!(pte.pa(), pa);
        assert_eq!(pte.flags(), expected_flags);
        assert!(pte.is_valid());
        assert!(pte.is_leaf());
    }

    for permissions in [
        0,
        PTE_W,
        PTE_W | PTE_X,
        PTE_R | PTE_V,
        PTE_R | PTE_A,
        PTE_R | 1 << 10,
        PTE_R | 1 << 4,
    ] {
        assert!(Pte::leaf(pa, permissions).is_none());
    }

    assert!(Pte::table(pa + 1).is_none());
    assert!(Pte::leaf(pa + 1, PTE_R).is_none());
    assert!(Pte::table(1usize << 56).is_none());
    assert!(Pte::leaf(1usize << 56, PTE_R).is_none());

    for pa in [
        0usize,
        0x1000_0000,
        1usize << 40,
        0x00ff_ffff_ffff_f000,
    ] {
        table = Pte::table(pa)
            .expect("valid PA rejected by table");

        assert_eq!(
            table.pa(),
            pa,
            "table PA round-trip failed: {:#x}",
            pa,
        );

        let leaf = Pte::leaf(pa, PTE_R)
            .expect("valid PA rejected by leaf");

        assert_eq!(
            leaf.pa(),
            pa,
            "leaf PA round-trip failed: {:#x}",
            pa,
        );
    }

    let _ = console::print(format_args!(
        "sv39: PTE checks passed\r\n",
    ));
}


pub fn check_page_walk() {
    use super::{
        pagetable::{self, read_pte, write_pte, translate_4k},
        vm::{self, Pte, PTE_R, PTE_W},
    };

    assert_eq!(
        core::mem::size_of::<Pte>() * pagetable::ENTRIES,
        PAGE_SIZE,
    );

    let before = pmem::free_counts();

    let root = pmem::alloc_kernel();
    let level1 = pmem::alloc_kernel();
    let level0 = pmem::alloc_kernel();
    let data = pmem::alloc_data();

    assert_eq!(
        pmem::free_counts(),
        (before.0 - 3, before.1 - 1),
    );

    let base = 0x4040_3000;
    let [vpn2, vpn1, vpn0] = 
        vm::vpn_indices(base).expect("invalid test VA");

    assert_eq!([vpn2, vpn1, vpn0], [1, 2, 3]);

    for pa in [
        root, level1, level0, data,
    ] {
        assert!(pa % PAGE_SIZE == 0);
        for entry in 0..ENTRIES {
            unsafe {
                assert!(read_pte(pa, entry).bits() == 0);
            }
        }
    }

    // SAFETY: 三张表由本函数独占，已清零且仍存活；尚未交给硬件
    unsafe {
        assert_eq!(translate_4k(root, base), None);
    }

    unsafe {
        write_pte(
            root,
            vpn2,
            Pte::table(level1).expect("level1 address"),
        );

        write_pte(
            level1,
            vpn1,
            Pte::table(level0).expect("level address"),
        );

        write_pte(
            level0,
            vpn0,
            Pte::leaf(data, PTE_R | PTE_W).expect("data address"),
        );
    }

    unsafe {
        for offset in [0, 0xabc, PAGE_SIZE - 1] {
            assert_eq!(
                translate_4k(root, base + offset),
                Some(data + offset),
            );
        }
    }

    unsafe {
        for offset in [
            base + PAGE_SIZE,
            base + (1 << 21),
            base + (1 << 30),
            0x0000_0040_0000_0000,
        ] {
            assert_eq!(
                translate_4k(root, offset),
                None,
            );
        }
    }

    unsafe {
        let translated = translate_4k(root, base + 0xabc)
            .expect("mapping missing");

        assert_eq!(translated, data + 0xabc);

        (data as *mut u8).add(0xabc).write(0x5a);

        assert_eq!(
            (translated as *const u8).read(),
            0x5a,
        );
    }

    unsafe {
        write_pte(level0, vpn0, Pte::empty());

        assert_eq!(translate_4k(root, base), None);

        write_pte(level1, vpn1, Pte::empty());
        write_pte(root, vpn2, Pte::empty());

        pmem::free_data(data);
        pmem::free_kernel(level0);
        pmem::free_kernel(level1);
        pmem::free_kernel(root);
    }

    assert_eq!(pmem::free_counts(), before);
    pmem::check_all_free();

    let _ = console::print(format_args!(
        "sv39: manual page walk checks passed\r\n",
    ));
}