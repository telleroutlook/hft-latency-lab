//! Order book — maintains bid/ask price levels using arena-allocated linked lists.
//! HashMap index for O(1) order_id lookup. Zero allocation in hot path.

use super::arena::{OrderArena, SENTINEL};
use std::collections::HashMap;

pub struct OrderBook {
    bids_head: u32,
    asks_head: u32,
    arena: OrderArena,
    /// O(1) lookup: order_id → arena index
    index: HashMap<u64, u32>,
    /// Callback invoked when BBO changes. (best_bid, best_ask)
    #[allow(clippy::type_complexity)]
    bbo_callback: Option<Box<dyn FnMut(Option<u64>, Option<u64>)>>,
}

impl OrderBook {
    pub fn new(arena_capacity: usize) -> Self {
        Self {
            bids_head: SENTINEL,
            asks_head: SENTINEL,
            arena: OrderArena::with_capacity(arena_capacity),
            index: HashMap::with_capacity(arena_capacity),
            bbo_callback: None,
        }
    }

    pub fn set_bbo_callback(&mut self, cb: Box<dyn FnMut(Option<u64>, Option<u64>)>) {
        self.bbo_callback = Some(cb);
    }

    pub fn add_order(&mut self, order_id: u64, is_buy: bool, price: u64, qty: u32) {
        if self.index.contains_key(&order_id) {
            return; // duplicate order_id, ignore
        }

        let idx = self.arena.alloc(price, qty, order_id);
        self.index.insert(order_id, idx);

        let old_best = if is_buy {
            self.best_bid()
        } else {
            self.best_ask()
        };

        if is_buy {
            self.insert_bid(idx);
        } else {
            self.insert_ask(idx);
        }

        let new_best = if is_buy {
            self.best_bid()
        } else {
            self.best_ask()
        };
        if old_best != new_best {
            let bid = self.best_bid();
            let ask = self.best_ask();
            if let Some(cb) = &mut self.bbo_callback {
                cb(bid, ask);
            }
        }
    }

    pub fn cancel_order(&mut self, order_id: u64) -> bool {
        if let Some(&idx) = self.index.get(&order_id) {
            let is_buy = self.is_on_bid_side(idx);

            let old_best = if is_buy {
                self.best_bid()
            } else {
                self.best_ask()
            };

            self.unlink(idx);
            self.arena.free(idx);
            self.index.remove(&order_id);

            let new_best = if is_buy {
                self.best_bid()
            } else {
                self.best_ask()
            };
            if old_best != new_best {
                let bid = self.best_bid();
                let ask = self.best_ask();
                if let Some(cb) = &mut self.bbo_callback {
                    cb(bid, ask);
                }
            }
            true
        } else {
            false
        }
    }

    /// Execute (partial fill) — reduce shares, remove if fully filled.
    pub fn execute_order(&mut self, order_id: u64, executed_shares: u32) -> bool {
        if let Some(&idx) = self.index.get(&order_id) {
            let current_qty = self.arena.get(idx).qty;
            if executed_shares >= current_qty {
                // Fully filled — remove order
                self.cancel_order(order_id);
            } else {
                // Partial fill — reduce quantity
                self.arena.get_mut(idx).qty -= executed_shares;
            }
            true
        } else {
            false
        }
    }

    /// Delete order (same as cancel in effect).
    pub fn delete_order(&mut self, order_id: u64) -> bool {
        self.cancel_order(order_id)
    }

    fn is_on_bid_side(&self, idx: u32) -> bool {
        let _price = self.arena.get(idx).price;
        // Walk bid side to check if this index is there
        let mut curr = self.bids_head;
        while curr != SENTINEL {
            if curr == idx {
                return true;
            }
            curr = self.arena.get(curr).next;
        }
        false
    }

    fn insert_bid(&mut self, idx: u32) {
        let price = self.arena.get(idx).price;
        if self.bids_head == SENTINEL {
            self.bids_head = idx;
            return;
        }
        let mut prev = SENTINEL;
        let mut curr = self.bids_head;
        while curr != SENTINEL {
            if self.arena.get(curr).price <= price {
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
        if self.bids_head == SENTINEL {
            return None;
        }
        Some(self.arena.get(self.bids_head).price)
    }

    pub fn best_ask(&self) -> Option<u64> {
        if self.asks_head == SENTINEL {
            return None;
        }
        Some(self.arena.get(self.asks_head).price)
    }

    pub fn spread(&self) -> Option<u64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn order_count(&self) -> usize {
        self.index.len()
    }

    /// Total quantity at best bid.
    pub fn best_bid_qty(&self) -> u32 {
        if self.bids_head == SENTINEL {
            return 0;
        }
        self.arena.get(self.bids_head).qty
    }

    /// Total quantity at best ask.
    pub fn best_ask_qty(&self) -> u32 {
        if self.asks_head == SENTINEL {
            return 0;
        }
        self.arena.get(self.asks_head).qty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn basic_order_book_operations() {
        let mut book = OrderBook::new(256);
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());

        book.add_order(1, true, 100, 10);
        book.add_order(2, true, 101, 20);
        book.add_order(3, false, 102, 15);

        assert_eq!(book.best_bid(), Some(101));
        assert_eq!(book.best_ask(), Some(102));
        assert_eq!(book.spread(), Some(1));
        assert_eq!(book.order_count(), 3);

        assert!(book.cancel_order(2));
        assert_eq!(book.best_bid(), Some(100));
        assert_eq!(book.order_count(), 2);
    }

    #[test]
    fn execute_partial_fill() {
        let mut book = OrderBook::new(256);
        book.add_order(1, true, 100, 50);

        // Partial fill
        assert!(book.execute_order(1, 20));
        assert_eq!(book.best_bid_qty(), 30);
        assert_eq!(book.order_count(), 1);

        // Full fill
        assert!(book.execute_order(1, 30));
        assert!(book.best_bid().is_none());
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn duplicate_order_ignored() {
        let mut book = OrderBook::new(256);
        book.add_order(1, true, 100, 10);
        book.add_order(1, true, 200, 20); // duplicate
        assert_eq!(book.best_bid(), Some(100));
        assert_eq!(book.order_count(), 1);
    }

    #[test]
    fn cancel_nonexistent_returns_false() {
        let mut book = OrderBook::new(256);
        assert!(!book.cancel_order(999));
    }

    #[test]
    fn bbo_callback_triggered() {
        static BBO_CHANGES: AtomicU64 = AtomicU64::new(0);

        let mut book = OrderBook::new(256);
        book.set_bbo_callback(Box::new(|bid, ask| {
            BBO_CHANGES.fetch_add(1, Ordering::SeqCst);
            let _ = (bid, ask);
        }));

        book.add_order(1, true, 100, 10); // new best bid
        book.add_order(2, true, 101, 10); // better bid → callback
        book.add_order(3, false, 102, 10); // new best ask → callback
        book.cancel_order(2); // best bid changes → callback

        assert_eq!(BBO_CHANGES.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn spread_calculation() {
        let mut book = OrderBook::new(256);
        assert!(book.spread().is_none());

        book.add_order(1, true, 100, 10);
        assert!(book.spread().is_none());

        book.add_order(2, false, 105, 10);
        assert_eq!(book.spread(), Some(5));
    }

    #[test]
    fn multi_level_book() {
        let mut book = OrderBook::new(1024);

        // Build a 5-level book on each side
        for i in 0..5 {
            book.add_order(100 + i as u64, true, 100 - i as u64, 10);
            book.add_order(200 + i as u64, false, 105 + i as u64, 10);
        }

        assert_eq!(book.best_bid(), Some(100));
        assert_eq!(book.best_ask(), Some(105));
        assert_eq!(book.order_count(), 10);

        // Cancel best bid
        book.cancel_order(100);
        assert_eq!(book.best_bid(), Some(99));
        assert_eq!(book.order_count(), 9);
    }
}
