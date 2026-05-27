//! Order book — maintains bid/ask price levels using arena-allocated linked lists.
//! Zero allocation in hot path: all nodes live in the arena.

use super::arena::{OrderArena, SENTINEL};

pub struct OrderBook {
    bids_head: u32,  // index of best bid
    asks_head: u32,  // index of best ask
    arena: OrderArena,
}

impl OrderBook {
    pub fn new(arena_capacity: usize) -> Self {
        Self {
            bids_head: SENTINEL,
            asks_head: SENTINEL,
            arena: OrderArena::with_capacity(arena_capacity),
        }
    }

    pub fn add_order(&mut self, order_id: u64, is_buy: bool, price: u64, qty: u32) {
        let idx = self.arena.alloc(price, qty, order_id);
        if is_buy {
            self.insert_bid(idx);
        } else {
            self.insert_ask(idx);
        }
    }

    pub fn cancel_order(&mut self, order_id: u64) -> bool {
        // Linear scan — fine for a training ground, optimize with hashmap later
        let mut found = SENTINEL;
        for i in 0..self.arena.len() {
            if self.arena.get(i as u32).order_id == order_id {
                found = i as u32;
                break;
            }
        }
        if found == SENTINEL {
            return false;
        }
        self.unlink(found);
        self.arena.free(found);
        true
    }

    fn insert_bid(&mut self, idx: u32) {
        let price = self.arena.get(idx).price;
        if self.bids_head == SENTINEL {
            self.bids_head = idx;
            return;
        }
        // Insert sorted by price descending (best bid first)
        // TODO: for price-level bucket, use sorted array instead of linked list
        let mut prev = SENTINEL;
        let mut curr = self.bids_head;
        while curr != SENTINEL {
            if self.arena.get(curr).price <= price {
                break;
            }
            prev = curr;
            curr = self.arena.get(curr).next;
        }
        // Insert between prev and curr
        self.arena.get_mut(idx).prev = prev;
        self.arena.get_mut(idx).next = curr;
        if prev != SENTINEL {
            self.arena.get_mut(prev).next = idx;
        } else {
            self.bids_head = idx;
        }
        if curr != SENTINEL {
            self.arena.get_mut(curr).prev = idx;
        }
    }

    fn insert_ask(&mut self, idx: u32) {
        let price = self.arena.get(idx).price;
        if self.asks_head == SENTINEL {
            self.asks_head = idx;
            return;
        }
        // Insert sorted by price ascending (best ask first)
        let mut prev = SENTINEL;
        let mut curr = self.asks_head;
        while curr != SENTINEL {
            if self.arena.get(curr).price >= price {
                break;
            }
            prev = curr;
            curr = self.arena.get(curr).next;
        }
        self.arena.get_mut(idx).prev = prev;
        self.arena.get_mut(idx).next = curr;
        if prev != SENTINEL {
            self.arena.get_mut(prev).next = idx;
        } else {
            self.asks_head = idx;
        }
        if curr != SENTINEL {
            self.arena.get_mut(curr).prev = idx;
        }
    }

    fn unlink(&mut self, idx: u32) {
        let node = self.arena.get(idx);
        let prev = node.prev;
        let next = node.next;
        if prev != SENTINEL {
            self.arena.get_mut(prev).next = next;
        } else {
            // Check which side this node is on
            if self.bids_head == idx {
                self.bids_head = next;
            } else if self.asks_head == idx {
                self.asks_head = next;
            }
        }
        if next != SENTINEL {
            self.arena.get_mut(next).prev = prev;
        }
    }

    pub fn best_bid(&self) -> Option<u64> {
        if self.bids_head == SENTINEL { return None; }
        Some(self.arena.get(self.bids_head).price)
    }

    pub fn best_ask(&self) -> Option<u64> {
        if self.asks_head == SENTINEL { return None; }
        Some(self.arena.get(self.asks_head).price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_order_book_operations() {
        let mut book = OrderBook::new(256);
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());

        book.add_order(1, true, 100, 10);
        book.add_order(2, true, 101, 20);
        book.add_order(3, false, 102, 15);

        assert_eq!(book.best_bid(), Some(101));  // highest bid
        assert_eq!(book.best_ask(), Some(102));  // lowest ask

        assert!(book.cancel_order(2));
        assert_eq!(book.best_bid(), Some(100));
    }
}
