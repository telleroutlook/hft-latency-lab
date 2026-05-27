//! Golden-reference ITCH 5.0 parser — slow but obviously correct.
//! Never optimize this. Its value is correctness, not speed.
//! Differential testing compares optimized parser output against this.
//!
//! All ITCH 5.0 integer fields are big-endian binary (NOT ASCII).
//! Prices are 4-byte fixed-point (price * 10000).
//! Timestamps are 6-byte nanoseconds since midnight.
//! Alpha fields are ASCII, left-justified, right-padded with spaces.

// ---------- Message structs ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemEvent {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub event_code: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketParticipantPosition {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub mpid: [u8; 4],
    pub stock: [u8; 8],
    pub primary_market_maker: bool,
    pub market_maker_mode: u8,
    pub market_participant_state: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddOrder {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub order_ref: u64,
    pub buy: bool,
    pub shares: u32,
    pub stock: [u8; 8],
    pub price: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddOrderMpid {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub order_ref: u64,
    pub buy: bool,
    pub shares: u32,
    pub stock: [u8; 8],
    pub price: u32,
    pub attribution: [u8; 4],
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
pub struct OrderExecutedWithPrice {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub order_ref: u64,
    pub executed_shares: u32,
    pub match_number: u64,
    pub printable: bool,
    pub execution_price: u32,
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
pub struct Trade {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub order_ref: u64,
    pub buy: bool,
    pub shares: u32,
    pub stock: [u8; 8],
    pub price: u32,
    pub match_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossTrade {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub shares: u64,
    pub stock: [u8; 8],
    pub cross_price: u32,
    pub match_number: u64,
    pub cross_type: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenTrade {
    pub stock_locate: u16,
    pub tracking_number: u16,
    pub timestamp_ns: u64,
    pub match_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    SystemEvent(SystemEvent),
    MarketParticipantPosition(MarketParticipantPosition),
    AddOrder(AddOrder),
    AddOrderMpid(AddOrderMpid),
    OrderExecuted(OrderExecuted),
    OrderExecutedWithPrice(OrderExecutedWithPrice),
    OrderCancel(OrderCancel),
    OrderDelete(OrderDelete),
    Trade(Trade),
    CrossTrade(CrossTrade),
    BrokenTrade(BrokenTrade),
    Unknown { msg_type: u8 },
}

// ---------- Helpers ----------

#[inline]
fn read_u16(buf: &[u8]) -> u16 {
    u16::from_be_bytes([buf[0], buf[1]])
}

#[inline]
fn read_u32(buf: &[u8]) -> u32 {
    u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

#[inline]
fn read_u64(buf: &[u8]) -> u64 {
    u64::from_be_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]])
}

/// Read 6-byte big-endian timestamp (nanoseconds since midnight).
#[inline]
fn read_timestamp(buf: &[u8]) -> u64 {
    // 6 bytes big-endian → u64
    let b = &buf[..6];
    ((b[0] as u64) << 40) | ((b[1] as u64) << 32) | ((b[2] as u64) << 24)
        | ((b[3] as u64) << 16) | ((b[4] as u64) << 8) | (b[5] as u64)
}

#[inline]
fn read_stock(buf: &[u8]) -> [u8; 8] {
    let mut s = [b' '; 8];
    s.copy_from_slice(&buf[..8]);
    s
}

/// Message sizes (payload only, without the 2-byte length prefix).
/// These are the total sizes as per ITCH 5.0 spec.
const MSG_SIZE_S: usize = 12;
const MSG_SIZE_L: usize = 26;
const MSG_SIZE_A: usize = 36;
const MSG_SIZE_F: usize = 40;
const MSG_SIZE_E: usize = 31;
const MSG_SIZE_C: usize = 36;
const MSG_SIZE_X: usize = 23;
const MSG_SIZE_D: usize = 19;
const MSG_SIZE_P: usize = 44;
const MSG_SIZE_Q: usize = 40;
const MSG_SIZE_B: usize = 19;

/// Parse a single ITCH 5.0 message from a byte slice.
/// Returns (parsed_message, bytes_consumed) or None if buffer too short.
pub fn parse_one(buf: &[u8]) -> Option<(Message, usize)> {
    if buf.is_empty() {
        return None;
    }

    let msg_type = buf[0];
    match msg_type {
        b'S' => {
            if buf.len() < MSG_SIZE_S { return None; }
            let msg = SystemEvent {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                event_code: buf[11],
            };
            Some((Message::SystemEvent(msg), MSG_SIZE_S))
        }
        b'L' => {
            if buf.len() < MSG_SIZE_L { return None; }
            let mut mpid = [b' '; 4];
            mpid.copy_from_slice(&buf[11..15]);
            let msg = MarketParticipantPosition {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                mpid,
                stock: read_stock(&buf[15..23]),
                primary_market_maker: buf[23] == b'Y',
                market_maker_mode: buf[24],
                market_participant_state: buf[25],
            };
            Some((Message::MarketParticipantPosition(msg), MSG_SIZE_L))
        }
        b'A' => {
            if buf.len() < MSG_SIZE_A { return None; }
            let msg = AddOrder {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                order_ref: read_u64(&buf[11..19]),
                buy: buf[19] == b'B',
                shares: read_u32(&buf[20..24]),
                stock: read_stock(&buf[24..32]),
                price: read_u32(&buf[32..36]),
            };
            Some((Message::AddOrder(msg), MSG_SIZE_A))
        }
        b'F' => {
            if buf.len() < MSG_SIZE_F { return None; }
            let mut attr = [b' '; 4];
            attr.copy_from_slice(&buf[36..40]);
            let msg = AddOrderMpid {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                order_ref: read_u64(&buf[11..19]),
                buy: buf[19] == b'B',
                shares: read_u32(&buf[20..24]),
                stock: read_stock(&buf[24..32]),
                price: read_u32(&buf[32..36]),
                attribution: attr,
            };
            Some((Message::AddOrderMpid(msg), MSG_SIZE_F))
        }
        b'E' => {
            if buf.len() < MSG_SIZE_E { return None; }
            let msg = OrderExecuted {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                order_ref: read_u64(&buf[11..19]),
                executed_shares: read_u32(&buf[19..23]),
                match_number: read_u64(&buf[23..31]),
            };
            Some((Message::OrderExecuted(msg), MSG_SIZE_E))
        }
        b'C' => {
            if buf.len() < MSG_SIZE_C { return None; }
            let msg = OrderExecutedWithPrice {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                order_ref: read_u64(&buf[11..19]),
                executed_shares: read_u32(&buf[19..23]),
                match_number: read_u64(&buf[23..31]),
                printable: buf[31] == b'Y',
                execution_price: read_u32(&buf[32..36]),
            };
            Some((Message::OrderExecutedWithPrice(msg), MSG_SIZE_C))
        }
        b'X' => {
            if buf.len() < MSG_SIZE_X { return None; }
            let msg = OrderCancel {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                order_ref: read_u64(&buf[11..19]),
                canceled_shares: read_u32(&buf[19..23]),
            };
            Some((Message::OrderCancel(msg), MSG_SIZE_X))
        }
        b'D' => {
            if buf.len() < MSG_SIZE_D { return None; }
            let msg = OrderDelete {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                order_ref: read_u64(&buf[11..19]),
            };
            Some((Message::OrderDelete(msg), MSG_SIZE_D))
        }
        b'P' => {
            if buf.len() < MSG_SIZE_P { return None; }
            let msg = Trade {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                order_ref: read_u64(&buf[11..19]),
                buy: buf[19] == b'B',
                shares: read_u32(&buf[20..24]),
                stock: read_stock(&buf[24..32]),
                price: read_u32(&buf[32..36]),
                match_number: read_u64(&buf[36..44]),
            };
            Some((Message::Trade(msg), MSG_SIZE_P))
        }
        b'Q' => {
            if buf.len() < MSG_SIZE_Q { return None; }
            let msg = CrossTrade {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                shares: read_u64(&buf[11..19]),
                stock: read_stock(&buf[19..27]),
                cross_price: read_u32(&buf[27..31]),
                match_number: read_u64(&buf[31..39]),
                cross_type: buf[39],
            };
            Some((Message::CrossTrade(msg), MSG_SIZE_Q))
        }
        b'B' => {
            if buf.len() < MSG_SIZE_B { return None; }
            let msg = BrokenTrade {
                stock_locate: read_u16(&buf[1..3]),
                tracking_number: read_u16(&buf[3..5]),
                timestamp_ns: read_timestamp(&buf[5..11]),
                match_number: read_u64(&buf[11..19]),
            };
            Some((Message::BrokenTrade(msg), MSG_SIZE_B))
        }
        _ => Some((Message::Unknown { msg_type }, 1)),
    }
}

/// Parse all messages from a byte buffer. Returns vector of parsed messages.
/// ITCH messages are length-prefixed: 2-byte big-endian length, then payload.
pub fn parse_all(buf: &[u8]) -> Vec<Message> {
    let mut msgs = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
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
    fn parse_add_order_binary() {
        let mut msg = Vec::new();
        msg.push(b'A');
        msg.extend_from_slice(&1u16.to_be_bytes());      // stock locate
        msg.extend_from_slice(&1u16.to_be_bytes());      // tracking number
        // 6-byte timestamp: 3600000000000 ns (= 10:00:00)
        let ts: u64 = 36_000_000_000_000;
        msg.extend_from_slice(&ts.to_be_bytes()[2..8]);  // take lower 6 bytes
        msg.extend_from_slice(&42u64.to_be_bytes());     // order ref
        msg.push(b'B');                                   // buy
        msg.extend_from_slice(&100u32.to_be_bytes());    // shares
        msg.extend_from_slice(b"TEST    ");               // stock (8 bytes)
        msg.extend_from_slice(&50000u32.to_be_bytes());  // price = 5.0000

        let (parsed, consumed) = parse_one(&msg).unwrap();
        assert_eq!(consumed, 36);
        match parsed {
            Message::AddOrder(a) => {
                assert_eq!(a.stock_locate, 1);
                assert_eq!(a.order_ref, 42);
                assert!(a.buy);
                assert_eq!(a.shares, 100);
                assert_eq!(a.price, 50000);
                assert_eq!(&a.stock, b"TEST    ");
            }
            _ => panic!("expected AddOrder"),
        }
    }

    #[test]
    fn parse_order_delete_binary() {
        let mut msg = Vec::new();
        msg.push(b'D');
        msg.extend_from_slice(&1u16.to_be_bytes());
        msg.extend_from_slice(&1u16.to_be_bytes());
        let ts: u64 = 36_000_000_000_000;
        msg.extend_from_slice(&ts.to_be_bytes()[2..8]);
        msg.extend_from_slice(&99u64.to_be_bytes());

        let (parsed, consumed) = parse_one(&msg).unwrap();
        assert_eq!(consumed, 19);
        match parsed {
            Message::OrderDelete(d) => assert_eq!(d.order_ref, 99),
            _ => panic!("expected OrderDelete"),
        }
    }

    #[test]
    fn parse_unknown_type_returns_unknown() {
        let buf = [b'Z'];
        let (parsed, consumed) = parse_one(&buf).unwrap();
        assert_eq!(consumed, 1);
        match parsed {
            Message::Unknown { msg_type } => assert_eq!(msg_type, b'Z'),
            _ => panic!("expected Unknown"),
        }
    }

    #[test]
    fn parse_too_short_returns_none() {
        let buf = [b'A', 0]; // too short for Add Order
        assert!(parse_one(&buf).is_none());
    }

    #[test]
    fn parse_all_length_prefixed() {
        // Build a stream: one Add Order + one Order Delete, each length-prefixed
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

        let mut del_msg = Vec::new();
        del_msg.push(b'D');
        del_msg.extend_from_slice(&1u16.to_be_bytes());
        del_msg.extend_from_slice(&1u16.to_be_bytes());
        del_msg.extend_from_slice(&0u64.to_be_bytes()[2..8]);
        del_msg.extend_from_slice(&1u64.to_be_bytes());

        let mut stream = Vec::new();
        let add_len = add_msg.len() as u16;
        stream.extend_from_slice(&add_len.to_be_bytes());
        stream.extend_from_slice(&add_msg);
        let del_len = del_msg.len() as u16;
        stream.extend_from_slice(&del_len.to_be_bytes());
        stream.extend_from_slice(&del_msg);

        let msgs = parse_all(&stream);
        assert_eq!(msgs.len(), 2);
    }
}
