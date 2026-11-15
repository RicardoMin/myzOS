use crate::spinlock::SpinLock;

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
            nodes: nodes,
            free_head: Some(NodeId(0)),
            free_count: N_MMAP,
        }
    }

    pub(crate) fn alloc(&mut self) -> NodeId {
        let id = self.free_head.expect("mmap node pool exhausted");
        let node = &mut self.nodes[id.0];

        assert!(!node.used);

        self.free_head = node.free_next;
        node.free_next = None;
        node.region = Region::EMPTY;
        node.used = true;
        self.free_count -= 1;

        id
    }

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
}