//! Single-Producer Single-Consumer ring buffer (Disruptor-inspired).
//! Zero-allocation, lock-free for the SPSC case.
//! Warning: measure tail latency after adopting — lock-free can worsen p99.9.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::mem::MaybeUninit;

pub struct SpscRing<T, const CAP: usize> {
    buf: UnsafeCell<[MaybeUninit<T>; CAP]>,
    head: AtomicUsize,  // read position (consumer)
    tail: AtomicUsize,  // write position (producer)
}

unsafe impl<T: Send, const CAP: usize> Send for SpscRing<T, CAP> {}
unsafe impl<T: Send, const CAP: usize> Sync for SpscRing<T, CAP> {}

impl<T, const CAP: usize> SpscRing<T, CAP> {
    pub fn new() -> Self {
        assert!(CAP.is_power_of_two(), "CAP must be power of 2");
        Self {
            buf: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Producer: push an item. Returns false if full.
    pub fn push(&self, item: T) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail - head >= CAP {
            return false; // full
        }
        unsafe {
            let buf = &mut *self.buf.get();
            buf[tail & (CAP - 1)] = MaybeUninit::new(item);
        }
        self.tail.store(tail + 1, Ordering::Release);
        true
    }

    /// Consumer: pop an item. Returns None if empty.
    pub fn pop(&self) -> Option<T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head >= tail {
            return None; // empty
        }
        let item = unsafe {
            let buf = &*self.buf.get();
            buf[head & (CAP - 1)].assume_init_read()
        };
        self.head.store(head + 1, Ordering::Release);
        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spsc_basic() {
        let ring: SpscRing<u64, 4> = SpscRing::new();
        assert!(ring.push(42));
        assert!(ring.push(43));
        assert_eq!(ring.pop(), Some(42));
        assert_eq!(ring.pop(), Some(43));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn spsc_full() {
        let ring: SpscRing<u64, 2> = SpscRing::new();
        assert!(ring.push(1));
        assert!(ring.push(2));
        assert!(!ring.push(3)); // full
        assert_eq!(ring.pop(), Some(1));
        assert!(ring.push(3)); // now room
    }
}
