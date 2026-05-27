//! Golden-reference ITCH parser — slow but obviously correct.
//! Never optimize this. Its value is correctness, not speed.
//! Differential testing compares optimized parser output against this.
//!
//! ITCH 5.0 message types are fixed-length ASCII records.
//! This parser does field-by-field ASCII→integer conversion with no tricks.

/// Parsed ITCH message — fields common to all message types.
/// Specific message types extend this via the Message enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddOrder {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub order_ref: u64,
    pub buy: bool,
    pub shares: u32,
    pub stock: [u8; 8],
    pub price: u32,  // price * 10000 (fixed-point)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderExecuted {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub order_ref: u64,
    pub executed_shares: u32,
    pub match_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderCancel {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub order_ref: u64,
    pub canceled_shares: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderDelete {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub order_ref: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    AddOrder(AddOrder),
    OrderExecuted(OrderExecuted),
    OrderCancel(OrderCancel),
    OrderDelete(OrderDelete),
    // TODO: add remaining ITCH 5.0 message types as needed
    Unknown { msg_type: u8 },
}

/// Naive ASCII-to-integer: parse big-endian ASCII digits in a byte slice.
fn parse_ascii_u64(buf: &[u8]) -> u64 {
    let mut val: u64 = 0;
    for &b in buf {
        val = val * 10 + (b - b'0') as u64;
    }
    val
}

fn parse_ascii_u32(buf: &[u8]) -> u32 {
    parse_ascii_u64(buf) as u32
}

fn parse_ascii_u16(buf: &[u8]) -> u16 {
    parse_ascii_u64(buf) as u16
}

/// Parse timestamp from 6-byte field (hhmmss in nanoseconds representation).
/// ITCH timestamps are nanoseconds since midnight.
fn parse_timestamp(buf: &[u8]) -> u64 {
    parse_ascii_u64(buf)
}

/// Parse a single ITCH 5.0 message from a byte slice.
/// Returns (parsed_message, bytes_consumed).
pub fn parse_one(buf: &[u8]) -> Option<(Message, usize)> {
    if buf.is_empty() {
        return None;
    }

    let msg_type = buf[0];
    match msg_type {
        b'A' | b'F' => {
            // Add Order (A) / Add Order with MPID (F) — parse the common fields
            if buf.len() < 36 { return None; }
            let msg = AddOrder {
                stock_locate: parse_ascii_u16(&buf[1..3]),
                tracking_number: parse_ascii_u16(&buf[3..5]),
                timestamp_ns: parse_timestamp(&buf[5..11]),
                order_ref: parse_ascii_u64(&buf[11..19]),
                buy: buf[19] == b'B',
                shares: parse_ascii_u32(&buf[20..26]),
                stock: {
                    let mut s = [b' '; 8];
                    s.copy_from_slice(&buf[28..36]);
                    s
                },
                price: parse_ascii_u32(&buf[36..46].get(..10).unwrap_or_default()),
            };
            let len = if msg_type == b'A' { 36 } else { 40 };
            Some((Message::AddOrder(msg), len.min(buf.len())))
        }
        b'E' => {
            // Order Executed
            if buf.len() < 33 { return None; }
            let msg = OrderExecuted {
                stock_locate: parse_ascii_u16(&buf[1..3]),
                tracking_number: parse_ascii_u16(&buf[3..5]),
                timestamp_ns: parse_timestamp(&buf[5..11]),
                order_ref: parse_ascii_u64(&buf[11..19]),
                executed_shares: parse_ascii_u32(&buf[19..25]),
                match_number: parse_ascii_u64(&buf[25..33]),
            };
            Some((Message::OrderExecuted(msg), 33))
        }
        b'X' => {
            // Order Cancel
            if buf.len() < 23 { return None; }
            let msg = OrderCancel {
                stock_locate: parse_ascii_u16(&buf[1..3]),
                tracking_number: parse_ascii_u16(&buf[3..5]),
                timestamp_ns: parse_timestamp(&buf[5..11]),
                order_ref: parse_ascii_u64(&buf[11..19]),
                canceled_shares: parse_ascii_u32(&buf[19..23]),
            };
            Some((Message::OrderCancel(msg), 23))
        }
        b'D' => {
            // Order Delete
            if buf.len() < 19 { return None; }
            let msg = OrderDelete {
                stock_locate: parse_ascii_u16(&buf[1..3]),
                tracking_number: parse_ascii_u16(&buf[3..5]),
                timestamp_ns: parse_timestamp(&buf[5..11]),
                order_ref: parse_ascii_u64(&buf[11..19]),
            };
            Some((Message::OrderDelete(msg), 19))
        }
        _ => Some((Message::Unknown { msg_type }, 1)),
    }
}

/// Parse all messages from a byte buffer. Returns vector of parsed messages.
pub fn parse_all(buf: &[u8]) -> Vec<Message> {
    let mut msgs = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        // ITCH messages are length-prefixed: 2-byte big-endian length, then payload
        if pos + 2 > buf.len() { break; }
        let msg_len = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
        let msg_start = pos + 2;
        let msg_end = msg_start + msg_len;
        if msg_end > buf.len() { break; }

        if let Some((msg, _)) = parse_one(&buf[msg_start..msg_end]) {
            msgs.push(msg);
        }
        pos = msg_end;
    }
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ascii_basic() {
        assert_eq!(parse_ascii_u64(b"12345"), 12345);
        assert_eq!(parse_ascii_u16(b"42"), 42);
        assert_eq!(parse_ascii_u32(b"000100"), 100);
    }
}
