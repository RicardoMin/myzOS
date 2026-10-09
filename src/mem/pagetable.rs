use super::{
    PAGE_SIZE,
    RAM_START,
    RAM_END,
    vm::{self, Pte},
};


pub const ENTRIES: usize = 512;


fn assert_slot(table_pa: usize, index: usize) {
    assert_eq!(table_pa % PAGE_SIZE, 0);

    assert!(
        RAM_START <= table_pa
            && table_pa <= RAM_END - PAGE_SIZE
    );

    assert!(index < ENTRIES);
}


/// # Safety
/// table_pa 指向仍然存活、已初始化的一整页页表；
/// 当前允许通过同值地址直接读取该物理页；
/// 读取期间没有并发修改、回收或冲突的可变引用
pub unsafe fn read_pte(
    table_pa: usize,
    index: usize,
) -> Pte {
    assert_slot(table_pa, index);

    unsafe {
        (table_pa as *const Pte).add(index).read()
    }
}


/// # Safety
/// table_pa 指向本调用者独占的、已初始化且仍存活的页表页；
/// 当前允许直接写入该物理页；
/// 没有其他执行者或冲突引用访问它，硬件也未使用这张表
pub unsafe fn write_pte(
    table_pa: usize,
    index: usize,
    entry: Pte,
) {
    assert_slot(table_pa, index);

    unsafe {
        (table_pa as *mut Pte).add(index).write(entry)
    }
}


/// # Safety
/// root 及查找过程中会跟随的非叶子条目，都指向存活
/// 已初始化、可通过同值地址直接读取的页表页；
/// 条目由当前 Pte 构造器或 empty 生成；
/// 整个查找期间没有冲突引用、并发修改或回收；
/// 这棵临时页表尚未交给硬件使用
pub unsafe fn translate_4k(
    root_pa: usize,
    va: usize,
) -> Option<usize> {
    let [vpn2, vpn1, vpn0] = vm::vpn_indices(va)?;

    let mut current = root_pa;

    for index in [vpn2, vpn1] {
        let pte = unsafe {
            read_pte(current, index)
        };

        if !pte.is_valid() {
            return None;
        }

        if pte.is_leaf() {
            return None;
        }

        current = pte.pa();
    }

    let pte = unsafe {
        read_pte(current, vpn0)
    };

    pte.is_leaf().then(|| {
        pte.pa() + (va & 0xfff)
    })
}