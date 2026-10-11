use super::{
    PAGE_SIZE,
    RAM_START,
    RAM_END,
    vm::{self, Pte},
    pmem,
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
        pte.pa() + vm::page_offset(va)
    })
}


#[derive(Debug, PartialEq, Eq)]
pub enum MapError {
    InvalidVa,
    InvalidLeaf,
    UpperLevelLeaf,
    SlotOccupied,
}


/// # Safety
/// 页表存活、初始化、独占、可直接访问；
/// 无并发修改或回收，硬件尚未使用；
/// 物理页分配器已经初始化
pub unsafe fn map_4k(
    root_pa: usize,
    va: usize,
    pa: usize,
    permissions: usize,
) -> Result<(), MapError> {
    let [vpn2, vpn1, vpn0] = 
        vm::vpn_indices(va).ok_or(MapError::InvalidVa)?;

    if vm::page_offset(va) != 0 {
        return Err(MapError::InvalidVa);
    }

    let leaf = 
        Pte::leaf(pa, permissions).ok_or(MapError::InvalidLeaf)?;

    let mut current = root_pa;

    for index in [vpn2, vpn1] {
        unsafe {
            let entry = read_pte(current, index);

            if !entry.is_valid() {
                let child = pmem::alloc_kernel();
                let link = Pte::table(child)
                    .expect("allocated page must be a valid page table address");
                write_pte(current, index, link);
                current = child;
            } else {
                (!Pte::is_leaf(entry)).ok_or(MapError::UpperLevelLeaf)?;
                current = entry.pa();
            }
        }
    }

    unsafe {
        let old = read_pte(current, vpn0);

        if old.is_valid() {
            return Err(MapError::SlotOccupied);
        }

        write_pte(current, vpn0, leaf);

        Ok(())
    }
}


#[derive(Debug, PartialEq, Eq)]
pub enum UnmapError {
    InvalidVa,
    NotMapped,
    UpperLevelLeaf,
    NotLeaf,
}


/// # Safety
/// table_pa 指向存活、已初始化且可直接读取的页表；
/// 扫描期间没有并发修改、回收或冲突引用
unsafe fn table_is_empty(table_pa: usize) -> bool {
    for index in 0..ENTRIES {
        let entry = unsafe { read_pte(table_pa, index) };

        if entry.is_valid() {
            return false;
        }
    }

    true
}


pub unsafe fn unmap_4k(
    root_pa: usize,
    va: usize,
) -> Result<Pte, UnmapError> {
    let indices = 
        vm::vpn_indices(va).ok_or(UnmapError::InvalidVa)?;

    if vm::page_offset(va) != 0 {
        return Err(UnmapError::InvalidVa);
    }

    let mut tables = [root_pa, 0, 0];

    for depth in 0..2 {
        let entry = unsafe { read_pte(tables[depth], indices[depth]) };

        if !entry.is_valid() {
            return Err(UnmapError::NotMapped);
        }

        if entry.is_leaf() {
            return Err(UnmapError::UpperLevelLeaf);
        }

        tables[depth + 1] = entry.pa();
    }

    let old = unsafe { read_pte(tables[2], indices[2])};

    if !old.is_valid() {
        return Err(UnmapError::NotMapped);
    }

    if !old.is_leaf() {
        return Err(UnmapError::NotLeaf);
    }

    unsafe {
        write_pte(tables[2], indices[2], Pte::empty());

        if table_is_empty(tables[2]) {
            write_pte(tables[1], indices[1], Pte::empty());
            pmem::free_kernel(tables[2]);

            if table_is_empty(tables[1]) {
                write_pte(tables[0], indices[0], Pte::empty());
                pmem::free_kernel(tables[1]);
            }
        }
    }

    return Ok(old);
}