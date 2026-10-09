use super::PAGE_SIZE;

const VPN_MASK: usize = (1usize << 9) - 1;

const LOWER_MAX: usize = (1usize << 38) - 1;
const UPPER_MIN: usize = !LOWER_MAX;


pub fn is_canonical(va: usize) -> bool {
    !(va > LOWER_MAX && va < UPPER_MIN)
}


pub fn page_offset(va: usize) -> usize {
    va & (PAGE_SIZE - 1)
}


/// 返回顺序固定为：[VPN2, VPN1, VPN0], 即从根到叶
pub fn vpn_indices(va: usize) -> Option<[usize; 3]> {
    is_canonical(va).then(|| {
        let vpn0 = (va >> 12) & VPN_MASK;
        let vpn1 = (va >> 21) & VPN_MASK;
        let vpn2 = (va >> 30) & VPN_MASK;
        [vpn2, vpn1, vpn0]
    })
}


pub const PTE_V: usize = 1 << 0;
pub const PTE_R: usize = 1 << 1;
pub const PTE_W: usize = 1 << 2;
pub const PTE_X: usize = 1 << 3;
pub const PTE_A: usize = 1 << 6;
pub const PTE_D: usize = 1 << 7;

const PPN_MASK: usize = (1usize << 44) - 1;
const FLAGS_MASK: usize = (1usize << 10) - 1;
const LEAF_PERMISSIONS: usize = PTE_R | PTE_W | PTE_X;


#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Pte(usize);


fn encode_pa(pa: usize) -> Option<usize> {
    if pa % PAGE_SIZE != 0 {
        return None;
    }

    let ppn = pa >> 12;

    (ppn <= PPN_MASK).then(|| {
        ppn << 10
    })
}


impl Pte {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn bits(self) -> usize {
        self.0
    }

    pub fn flags(self) -> usize {
        self.0 & FLAGS_MASK
    }

    pub fn is_valid(self) -> bool {
        self.0 & PTE_V != 0
    }

    pub fn is_leaf(self) -> bool {
        self.is_valid() && ((self.0 & PTE_X != 0) || (self.0 & PTE_R != 0))
    }

    pub fn pa(self) -> usize {
        (self.0 >> 10 & PPN_MASK) << 12
    }

    pub fn table(pa: usize) -> Option<Self> {
        let address_bits = encode_pa(pa)?;

        Some(Self(address_bits | PTE_V))
    }

    pub fn leaf(pa: usize, permissions: usize) -> Option<Self> {
        let address_bits = encode_pa(pa)?;

        if permissions == 0 {
            return None;
        }
        if permissions & !LEAF_PERMISSIONS != 0 {
            return None;
        }

        if (permissions & PTE_W) != 0 && (permissions & PTE_R) == 0 {
            return None;
        }

        let mut flags = permissions | PTE_V | PTE_A;

        if permissions & PTE_W != 0 {
            flags |= PTE_D;
        }

        Some(Self(address_bits | flags))
    }
}