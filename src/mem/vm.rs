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