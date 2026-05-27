//! Arena allocator for order book nodes — zero allocation in hot path.
//! Pre-allocate a fixed-size array of OrderNodes, use indices instead of pointers.
//! Cache-friendly: sequential index access = sequential memory access.

pub const SENTINEL: u32 = 0xFFFF_FFFF;

#[derive(Debug, Clone)]
pub struct OrderNode {
    pub price: u64,
    pub qty: u32,
    pub next: u32,   // index of next node, SENTINEL = null
    pub prev: u32,   // index of prev node, SENTINEL = null
    pub order_id: u64,
}

impl Default for OrderNode {
    fn default() -> Self {
        Self {
            price: 0,
            qty: 0,
            next: SENTINEL,
            prev: SENTINEL,
            order_id: 0,
        }
    }
}

pub struct OrderArena {
    nodes: Vec<OrderNode>,
    free_list: Vec<u32>,
}

impl OrderArena {
    pub fn with_capacity(n: usize) -> Self {
        let mut nodes = Vec::with_capacity(n);
        // Pre-fill with default nodes
        for _ in 0..n {
            nodes.push(OrderNode::default());
        }
        Self {
            nodes,
            free_list: Vec::with_capacity(n),
        }
    }

    /// Allocate a node. Returns its index.
    /// Hot path: pop from free list or bump allocate.
    pub fn alloc(&mut self, price: u64, qty: u32, order_id: u64) -> u32 {
        let idx = if let Some(idx) = self.free_list.pop() {
            idx
        } else {
            let idx = self.nodes.len() as u32;
            self.nodes.push(OrderNode::default());
            idx
        };
        self.nodes[idx as usize] = OrderNode {
            price,
            qty,
            next: SENTINEL,
            prev: SENTINEL,
            order_id,
        };
        idx
    }

    /// Free a node — push index back to free list for reuse.
    pub fn free(&mut self, idx: u32) {
        debug_assert!((idx as usize) < self.nodes.len());
        self.free_list.push(idx);
    }

    #[inline(always)]
    pub fn get(&self, idx: u32) -> &OrderNode {
        &self.nodes[idx as usize]
    }

    #[inline(always)]
    pub fn get_mut(&mut self, idx: u32) -> &mut OrderNode {
        &mut self.nodes[idx as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_alloc_free_cycle() {
        let mut arena = OrderArena::with_capacity(16);
        let a = arena.alloc(100, 10, 1);
        let b = arena.alloc(200, 20, 2);
        assert_eq!(arena.get(a).price, 100);
        assert_eq!(arena.get(b).price, 200);

        arena.free(a);
        let c = arena.alloc(300, 30, 3);
        assert_eq!(c, a, "should reuse freed slot");
        assert_eq!(arena.get(c).price, 300);
    }
}
