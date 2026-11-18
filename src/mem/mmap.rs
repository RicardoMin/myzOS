use crate::spinlock::SpinLock;
use super::{
    PAGE_SIZE,
    layout::{MMAP_BEGIN, MMAP_END},
};

pub(crate) mod selftest;

pub(crate) const N_MMAP: usize  = 256;


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NodeId(usize);


#[derive(Clone, Copy)]
pub(crate) struct Region {
    pub(crate) begin: usize,
    pub(crate) pages: usize,
    pub(crate) next: Option<NodeId>,
}

#[derive(Clone, Copy)]
struct Node {
    region: Region,
    free_next: Option<NodeId>,
    used: bool,
}

pub(crate) struct NodePool {
    nodes: [Node; N_MMAP],
    free_head: Option<NodeId>,
    free_count: usize,
}


pub(crate) static POOL: SpinLock<NodePool> =
    SpinLock::new(NodePool::new());


impl Region {
    const EMPTY: Self = Self {
        begin: 0,
        pages: 0,
        next: None,
    };
}


impl Node {
    const EMPTY: Self = Self {
        region: Region::EMPTY,
        free_next: None,
        used: false,
    };
}


impl NodeId {
    pub(crate) fn index(self) -> usize {
        self.0
    }
}


impl NodePool {
    const fn new() -> Self {
        let mut nodes = [Node::EMPTY; N_MMAP];

        let mut i = 0;
        while i + 1 < N_MMAP {
            nodes[i].free_next = Some(NodeId(i + 1));
            i += 1;
        }

        Self {
            nodes,
            free_head: Some(NodeId(0)),
            free_count: N_MMAP,
        }
    }

    pub(crate) fn alloc(&mut self) -> NodeId {
        let id = self.free_head.expect("mmap node pool exhausted");
        let node = &mut self.nodes[id.index()];

        assert!(!node.used);

        self.free_head = node.free_next;
        node.free_next = None;
        node.region = Region::EMPTY;
        node.used = true;
        self.free_count -= 1;

        id
    }

    /// 仅归还当前拥有且脱离区域链的节点；归还后不得再使用旧编号
    pub(crate) fn free(&mut self, id: NodeId) {
        let node = &mut self.nodes[id.0];
        assert!(node.used);

        node.used = false;
        node.free_next = self.free_head;
        self.free_head = Some(id);
        self.free_count += 1;
    }

    pub(crate) fn get(&self, id: NodeId) -> &Region {
        let node = &self.nodes[id.0];
        assert!(node.used);

        &node.region
    }

    pub(crate) fn get_mut(&mut self, id: NodeId) -> &mut Region {
        let node = &mut self.nodes[id.0];
        assert!(node.used);

        &mut node.region
    }

    pub(crate) fn free_count(&self) -> usize {
        self.free_count
    }

    fn find_gap(
        &self,
        head: Option<NodeId>,
        len: usize,
    ) -> Option<usize> {
        let mut candidate = MMAP_BEGIN;

        let mut next = head;

        while let Some(id) = next {
            let region = self.get(id);
            let region_len = region.begin - candidate;

            if region_len >= len {
                return Some(candidate);
            }

            candidate = region.begin + region.pages * PAGE_SIZE;
            next = region.next
        }

        if MMAP_END - candidate >= len {
            Some(candidate)
        } else {
            None
        }
    }

    pub(crate) fn reserve_region(
        &mut self,
        head: &mut Option<NodeId>,
        requested_begin: usize,
        len: usize,
    ) -> Option<usize> {
        if len == 0 {
            return None;
        }
        if len % PAGE_SIZE != 0 {
            return None;
        }
        if requested_begin % PAGE_SIZE != 0 {
            return None;
        }

        let begin = if requested_begin == 0 {
            self.find_gap(*head, len)?
        } else {
            requested_begin
        };

        let end = begin.checked_add(len)?;
        let pages = len / PAGE_SIZE;
        
        if begin < MMAP_BEGIN || end > MMAP_END {
            return None;
        }

        let mut prev = None;
        let mut next = *head;

        while let Some(id) = next {
            let region = self.get(id);
            if region.begin >= begin {
                break;
            }
            prev = Some(id);
            next = region.next;
        }

        let left_adjacent = if let Some(id) = prev {
            let left = self.get(id);
            let left_end = left.begin + left.pages * PAGE_SIZE;

            if left_end > begin {
                return None;
            }

            left_end == begin
        } else {
            false
        };

        let right_adjacent = if let Some(id) = next {
            let right = self.get(id);

            if end > right.begin {
                return None;
            }

            end == right.begin
        } else {
            false
        };

        match (left_adjacent, right_adjacent) {
            (false, false) => {
                let id = self.alloc();
                {
                    let region = self.get_mut(id);
                    region.begin = begin;
                    region.pages = pages;
                    region.next = next;
                }

                if let Some(left_id) = prev {
                    self.get_mut(left_id).next = Some(id);
                } else {
                    *head = Some(id);
                }
            }
            (true, false) => {
                let left_region = self.get_mut(prev.unwrap());
                left_region.pages += pages;
            }
            (false, true) => {
                let right_region = self.get_mut(next.unwrap());
                right_region.begin = begin;
                right_region.pages += pages;
            }
            (true, true) => {
                let right_id = next.unwrap();
                let right = *self.get(right_id);
                {
                    let left = self.get_mut(prev.unwrap());
                    left.pages += pages + right.pages;
                    left.next = right.next;
                }

                self.free(right_id);
            }
        }

        Some(begin)
    }
}