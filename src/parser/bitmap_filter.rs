//! Bitmap-based late materialization for ITCH message filtering.
//!
//! Ported from mini-vec-engine's bitmap module. Two-pass strategy:
//!   Pass 1: scan the buffer, set bitmap bits for messages matching target msg_type
//!   Pass 2: iterate set bits, parse only matching messages (skip everything else)
//!
//! This avoids branch mispredictions on non-matching message types during the
//! expensive parse phase — the same "late materialization" trick used in
//! vectorized query engines for predicate pushdown.

/// Fixed-width bitmap for message selection (512 bits covers ~512 messages per chunk).
/// Uses 8 x u64 words, same layout as mini-vec-engine's `Bitmap<W>`.
#[derive(Clone, Debug)]
pub struct MsgBitmap {
    words: [u64; 8],
}

impl MsgBitmap {
    pub const BITS: usize = 8 * 64; // 512

    pub fn zeroed() -> Self {
        Self { words: [0u64; 8] }
    }

    #[inline]
    pub fn set(&mut self, bit: usize) {
        debug_assert!(bit < Self::BITS);
        self.words[bit / 64] |= 1u64 << (bit % 64);
    }

    #[inline]
    pub fn get(&self, bit: usize) -> bool {
        debug_assert!(bit < Self::BITS);
        (self.words[bit / 64] >> (bit % 64)) & 1 == 1
    }

    pub fn popcount(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Iterate over all set bit positions (ascending order).
    /// Same algorithm as mini-vec-engine's `Bitmap::iter_set_bits()`.
    pub fn iter_set_bits(&self) -> impl Iterator<Item = usize> + '_ {
        self.words
            .iter()
            .enumerate()
            .filter(|(_, &word)| word != 0)
            .flat_map(|(word_idx, &word)| {
                let base = word_idx * 64;
                std::iter::successors(Some(word), move |&w| {
                    let next = w & (w - 1); // clear lowest set bit
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

/// First pass: scan the ITCH buffer and build a bitmap indicating which messages
/// match `target_type`. This pass only reads the 2-byte length prefix and the
/// first byte of each message (the msg_type field) — no full parsing occurs.
///
/// Returns the bitmap and a vector of `(msg_start_offset, msg_end_offset)` for
/// every message in the buffer (needed to locate messages by index in pass 2).
pub fn evaluate_msg_type(buf: &[u8], target_type: u8) -> (MsgBitmap, Vec<(usize, usize)>) {
    let mut bitmap = MsgBitmap::zeroed();
    let mut offsets = Vec::new();

    let mut pos = 0;
    let mut msg_idx = 0;
    let len = buf.len();

    while pos + 2 <= len && msg_idx < MsgBitmap::BITS {
        let msg_len = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
        let msg_start = pos + 2;
        let msg_end = msg_start + msg_len;
        if msg_end > len {
            break;
        }

        if msg_len > 0 && buf[msg_start] == target_type {
            bitmap.set(msg_idx);
        }
        offsets.push((msg_start, msg_end));

        pos = msg_end;
        msg_idx += 1;
    }

    (bitmap, offsets)
}

/// Second pass: parse only messages whose bitmap bit is set (late materialization).
/// Uses `iter_set_bits()` to efficiently skip non-matching messages.
///
/// Returns `(Message, byte_offset_of_message_start)` for each matching message.
pub fn filter_messages(
    buf: &[u8],
    bitmap: &MsgBitmap,
    offsets: &[(usize, usize)],
) -> Vec<(super::naive::Message, usize)> {
    use super::optimized::parse_one;

    let mut results = Vec::with_capacity(bitmap.popcount());
    for bit_idx in bitmap.iter_set_bits() {
        if bit_idx >= offsets.len() {
            break;
        }
        let (msg_start, msg_end) = offsets[bit_idx];
        if let Some((msg, _)) = parse_one(&buf[msg_start..msg_end]) {
            results.push((msg, msg_start));
        }
    }
    results
}

/// Convenience function: single-call bitmap-filter pipeline.
/// Scans `buf` for messages of `target_type`, returns fully parsed messages.
pub fn filter_by_type(buf: &[u8], target_type: u8) -> Vec<(super::naive::Message, usize)> {
    let (bitmap, offsets) = evaluate_msg_type(buf, target_type);
    filter_messages(buf, &bitmap, &offsets)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a length-prefixed ITCH message stream for testing.
    fn build_test_stream() -> Vec<u8> {
        let mut stream = Vec::new();

        // Add Order (type 'A')
        let mut add_msg = Vec::new();
        add_msg.push(b'A');
        add_msg.extend_from_slice(&1u16.to_be_bytes());
        add_msg.extend_from_slice(&1u16.to_be_bytes());
        add_msg.extend_from_slice(&0u64.to_be_bytes()[2..8]);
        add_msg.extend_from_slice(&1u64.to_be_bytes());
        add_msg.push(b'B');
        add_msg.extend_from_slice(&100u32.to_be_bytes());
        add_msg.extend_from_slice(b"GOOG    ");
        add_msg.extend_from_slice(&10000u32.to_be_bytes());
        let add_len = add_msg.len() as u16;
        stream.extend_from_slice(&add_len.to_be_bytes());
        stream.extend_from_slice(&add_msg);

        // Order Delete (type 'D')
        let mut del_msg = Vec::new();
        del_msg.push(b'D');
        del_msg.extend_from_slice(&1u16.to_be_bytes());
        del_msg.extend_from_slice(&1u16.to_be_bytes());
        del_msg.extend_from_slice(&0u64.to_be_bytes()[2..8]);
        del_msg.extend_from_slice(&99u64.to_be_bytes());
        let del_len = del_msg.len() as u16;
        stream.extend_from_slice(&del_len.to_be_bytes());
        stream.extend_from_slice(&del_msg);

        // Add Order (type 'A')
        let mut add_msg2 = Vec::new();
        add_msg2.push(b'A');
        add_msg2.extend_from_slice(&2u16.to_be_bytes());
        add_msg2.extend_from_slice(&1u16.to_be_bytes());
        add_msg2.extend_from_slice(&0u64.to_be_bytes()[2..8]);
        add_msg2.extend_from_slice(&2u64.to_be_bytes());
        add_msg2.push(b'S');
        add_msg2.extend_from_slice(&200u32.to_be_bytes());
        add_msg2.extend_from_slice(b"AAPL    ");
        add_msg2.extend_from_slice(&20000u32.to_be_bytes());
        let add_len2 = add_msg2.len() as u16;
        stream.extend_from_slice(&add_len2.to_be_bytes());
        stream.extend_from_slice(&add_msg2);

        stream
    }

    #[test]
    fn bitmap_set_get_clear() {
        let mut bm = MsgBitmap::zeroed();
        assert!(!bm.get(0));
        assert!(!bm.get(511));

        bm.set(0);
        bm.set(511);
        assert!(bm.get(0));
        assert!(bm.get(511));
        assert_eq!(bm.popcount(), 2);
    }

    #[test]
    fn bitmap_iter_set_bits() {
        let mut bm = MsgBitmap::zeroed();
        bm.set(0);
        bm.set(63);
        bm.set(64);
        bm.set(127);
        let bits: Vec<_> = bm.iter_set_bits().collect();
        assert_eq!(bits, vec![0, 63, 64, 127]);
    }

    #[test]
    fn evaluate_finds_matching_messages() {
        let stream = build_test_stream();
        let (bitmap, offsets) = evaluate_msg_type(&stream, b'A');
        assert_eq!(offsets.len(), 3);
        assert_eq!(bitmap.popcount(), 2);
        assert!(bitmap.get(0)); // first message is 'A'
        assert!(!bitmap.get(1)); // second message is 'D'
        assert!(bitmap.get(2)); // third message is 'A'
    }

    #[test]
    fn filter_returns_only_matching() {
        let stream = build_test_stream();
        let results = filter_by_type(&stream, b'A');
        assert_eq!(results.len(), 2);
        for (msg, _) in &results {
            matches!(msg, super::super::naive::Message::AddOrder(_));
        }
    }

    #[test]
    fn filter_empty_when_no_match() {
        let stream = build_test_stream();
        let results = filter_by_type(&stream, b'Z');
        assert!(results.is_empty());
    }
}
