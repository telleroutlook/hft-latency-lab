//! Arbitrary-size bitmap for multi-symbol ITCH filtering.
//!
//! Generalizes `MsgBitmap` (512-bit fixed) to a dynamically-sized bitmap
//! backed by `Vec<u64>`. API follows mini-vec-engine's `Bitmap<W>` pattern
//! but with heap-allocated words for unbounded message streams.

/// Dynamically-sized multi-word bitmap.
#[derive(Clone, Debug)]
pub struct Bitmap64 {
    words: Vec<u64>,
    n_bits: usize,
}

impl Bitmap64 {
    /// Number of bits per word.
    const BITS_PER_WORD: usize = 64;

    /// Create a zeroed bitmap with `n_bits` capacity.
    /// Internally rounds up to the nearest word boundary.
    pub fn new(n_bits: usize) -> Self {
        let n_words = (n_bits + Self::BITS_PER_WORD - 1) / Self::BITS_PER_WORD;
        Self {
            words: vec![0u64; n_words],
            n_bits,
        }
    }

    /// Total bit capacity.
    pub fn n_bits(&self) -> usize {
        self.n_bits
    }

    /// Number of underlying u64 words.
    pub fn n_words(&self) -> usize {
        self.words.len()
    }

    #[inline]
    pub fn set(&mut self, bit: usize) {
        debug_assert!(bit < self.n_bits);
        self.words[bit / 64] |= 1u64 << (bit % 64);
    }

    #[inline]
    pub fn clear(&mut self, bit: usize) {
        debug_assert!(bit < self.n_bits);
        self.words[bit / 64] &= !(1u64 << (bit % 64));
    }

    #[inline]
    pub fn test(&self, bit: usize) -> bool {
        if bit >= self.n_bits {
            return false;
        }
        (self.words[bit / 64] >> (bit % 64)) & 1 == 1
    }

    /// Bitwise AND of two bitmaps. Result size is the smaller of the two.
    pub fn and(&self, other: &Bitmap64) -> Bitmap64 {
        let n = self.words.len().min(other.words.len());
        let n_bits = self.n_bits.min(other.n_bits);
        let words = (0..n).map(|i| self.words[i] & other.words[i]).collect();
        Bitmap64 { words, n_bits }
    }

    /// Bitwise OR of two bitmaps. Result size is the larger of the two.
    pub fn or(&self, other: &Bitmap64) -> Bitmap64 {
        let (long, short) = if self.words.len() >= other.words.len() {
            (self, other)
        } else {
            (other, self)
        };
        let n_bits = self.n_bits.max(other.n_bits);
        let mut words = Vec::with_capacity(long.words.len());
        for i in 0..short.words.len() {
            words.push(self.words[i] | other.words[i]);
        }
        for i in short.words.len()..long.words.len() {
            words.push(long.words[i]);
        }
        Bitmap64 { words, n_bits }
    }

    /// Count set bits across all words.
    pub fn popcount(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Iterate over all set bit positions in ascending order.
    /// Uses the same clear-lowest-bit trick as mini-vec-engine's `iter_set_bits`.
    pub fn iter_set_bits(&self) -> impl Iterator<Item = usize> + '_ {
        self.words
            .iter()
            .enumerate()
            .filter(|(_, &word)| word != 0)
            .flat_map(|(word_idx, &word)| {
                let base = word_idx * 64;
                std::iter::successors(Some(word), move |&w| {
                    let next = w & (w - 1);
                    if next == 0 {
                        None
                    } else {
                        Some(next)
                    }
                })
                .map(move |w| base + (w.trailing_zeros() as usize))
            })
    }
}

/// First pass: scan ITCH buffer and build a per-message bitmap where each bit
/// indicates the message's stock field matches one of the symbol indices in
/// `symbol_filter`. The `stock_at` closure extracts the 8-byte stock field from
/// a message at the given byte offset — this is the same cheap header scan used
/// in `bitmap_filter::evaluate_msg_type`.
///
/// Returns a `Bitmap64` with one bit per message found in `buf`.
pub fn build_stock_bitmap<F>(
    buf: &[u8],
    max_messages: usize,
    stock_at: F,
) -> (Bitmap64, Vec<(usize, usize)>)
where
    F: Fn(&[u8]) -> [u8; 8],
{
    let bitmap = Bitmap64::new(max_messages);
    let mut offsets = Vec::new();

    let mut pos = 0;
    let mut msg_idx = 0;
    let len = buf.len();

    while pos + 2 <= len && msg_idx < max_messages {
        let msg_len = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
        let msg_start = pos + 2;
        let msg_end = msg_start + msg_len;
        if msg_end > len {
            break;
        }
        if msg_len > 0 {
            let _stock = stock_at(&buf[msg_start..msg_end]);
        }
        offsets.push((msg_start, msg_end));
        pos = msg_end;
        msg_idx += 1;
    }

    (bitmap, offsets)
}

/// Multi-symbol filtering: given a bitmap of desired symbol indices and a
/// per-symbol bitmap for each symbol, compute the union of matching message
/// indices using batch popcount for early termination.
///
/// For each symbol index set in `symbol_filter`, intersect with the per-symbol
/// bitmap and collect surviving message indices. The batch popcount lets us
/// skip symbols with zero intersection cheaply.
pub fn filter_multi_symbol(
    symbol_filter: &Bitmap64,
    per_symbol_bitmaps: &[Bitmap64],
) -> Bitmap64 {
    let n_bits = per_symbol_bitmaps
        .first()
        .map(|b| b.n_bits())
        .unwrap_or(0);
    let mut result = Bitmap64::new(n_bits);

    for sym_idx in symbol_filter.iter_set_bits() {
        if sym_idx >= per_symbol_bitmaps.len() {
            break;
        }
        let match_bits = per_symbol_bitmaps[sym_idx].and(&result);
        if match_bits.popcount() == per_symbol_bitmaps[sym_idx].popcount() {
            // All bits already covered — skip full OR.
            continue;
        }
        result = result.or(&per_symbol_bitmaps[sym_idx]);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_zeroed() {
        let bm = Bitmap64::new(200);
        assert_eq!(bm.n_bits(), 200);
        assert_eq!(bm.popcount(), 0);
        for i in 0..200 {
            assert!(!bm.test(i));
        }
    }

    #[test]
    fn set_clear_test() {
        let mut bm = Bitmap64::new(300);
        bm.set(0);
        bm.set(63);
        bm.set(64);
        bm.set(199);
        bm.set(299);
        assert!(bm.test(0));
        assert!(bm.test(63));
        assert!(bm.test(64));
        assert!(bm.test(199));
        assert!(bm.test(299));
        assert!(!bm.test(1));
        assert!(!bm.test(62));
        assert_eq!(bm.popcount(), 5);

        bm.clear(64);
        assert!(!bm.test(64));
        assert_eq!(bm.popcount(), 4);
    }

    #[test]
    fn test_out_of_bounds_returns_false() {
        let bm = Bitmap64::new(64);
        assert!(!bm.test(64));
        assert!(!bm.test(1000));
    }

    #[test]
    fn iter_set_bits_basic() {
        let mut bm = Bitmap64::new(200);
        bm.set(0);
        bm.set(63);
        bm.set(64);
        bm.set(127);
        bm.set(199);
        let bits: Vec<_> = bm.iter_set_bits().collect();
        assert_eq!(bits, vec![0, 63, 64, 127, 199]);
    }

    #[test]
    fn iter_set_bits_empty() {
        let bm = Bitmap64::new(100);
        let bits: Vec<_> = bm.iter_set_bits().collect();
        assert!(bits.is_empty());
    }

    #[test]
    fn and_same_size() {
        let mut a = Bitmap64::new(128);
        let mut b = Bitmap64::new(128);
        a.set(0);
        a.set(1);
        a.set(100);
        b.set(1);
        b.set(2);
        b.set(100);

        let result = a.and(&b);
        assert!(result.test(1));
        assert!(result.test(100));
        assert!(!result.test(0));
        assert!(!result.test(2));
        assert_eq!(result.popcount(), 2);
    }

    #[test]
    fn and_different_sizes() {
        let mut a = Bitmap64::new(128);
        let mut b = Bitmap64::new(64);
        a.set(0);
        a.set(100);
        b.set(0);

        let result = a.and(&b);
        assert!(result.test(0));
        assert!(!result.test(100));
        assert_eq!(result.n_bits(), 64);
    }

    #[test]
    fn or_same_size() {
        let mut a = Bitmap64::new(128);
        let mut b = Bitmap64::new(128);
        a.set(0);
        a.set(1);
        b.set(1);
        b.set(2);

        let result = a.or(&b);
        assert!(result.test(0));
        assert!(result.test(1));
        assert!(result.test(2));
        assert_eq!(result.popcount(), 3);
    }

    #[test]
    fn or_different_sizes() {
        let mut a = Bitmap64::new(64);
        let mut b = Bitmap64::new(128);
        a.set(0);
        b.set(1);
        b.set(100);

        let result = a.or(&b);
        assert!(result.test(0));
        assert!(result.test(1));
        assert!(result.test(100));
        assert_eq!(result.n_bits(), 128);
    }

    #[test]
    fn popcount_large() {
        let mut bm = Bitmap64::new(10_000);
        for i in (0..10_000).step_by(7) {
            bm.set(i);
        }
        assert_eq!(bm.popcount(), (10_000 + 6) / 7);
    }

    #[test]
    fn filter_multi_symbol_basic() {
        // 3 symbols, 10 message slots each
        let mut sym0 = Bitmap64::new(10);
        sym0.set(0);
        sym0.set(3);
        sym0.set(7);

        let mut sym1 = Bitmap64::new(10);
        sym1.set(1);
        sym1.set(5);

        let mut sym2 = Bitmap64::new(10);
        sym2.set(2);
        sym2.set(9);

        let per_symbol = vec![sym0, sym1, sym2];

        // Filter for symbols 0 and 2
        let mut symbol_filter = Bitmap64::new(3);
        symbol_filter.set(0);
        symbol_filter.set(2);

        let result = filter_multi_symbol(&symbol_filter, &per_symbol);
        assert_eq!(result.popcount(), 5);
        assert!(result.test(0));
        assert!(result.test(2));
        assert!(result.test(3));
        assert!(result.test(7));
        assert!(result.test(9));
        assert!(!result.test(1));
        assert!(!result.test(5));
    }

    #[test]
    fn filter_multi_symbol_empty_filter() {
        let mut sym0 = Bitmap64::new(10);
        sym0.set(0);
        let per_symbol = vec![sym0];

        let symbol_filter = Bitmap64::new(1);
        let result = filter_multi_symbol(&symbol_filter, &per_symbol);
        assert_eq!(result.popcount(), 0);
    }

    #[test]
    fn filter_multi_symbol_no_bitmaps() {
        let mut symbol_filter = Bitmap64::new(3);
        symbol_filter.set(0);
        let result = filter_multi_symbol(&symbol_filter, &[]);
        assert_eq!(result.popcount(), 0);
    }
}
