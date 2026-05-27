//! Optimized ITCH 5.0 parser — measurement-driven optimization.
//!
//! Optimizations applied (each verified by differential testing):
//! 1. ✅ Unchecked buffer reads with bounds check hoisting per message type
//! 2. ✅ Pre-allocated output vector with capacity hint
//! 3. ✅ Inlined field readers with #[inline(always)]
//! 4. ✅ Direct pattern match on msg_type (no indirection through naive)

use super::naive::Message;

#[inline(always)]
fn read_u16(buf: &[u8]) -> u16 {
    u16::from_be_bytes([buf[0], buf[1]])
}

#[inline(always)]
fn read_u32(buf: &[u8]) -> u32 {
    u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

#[inline(always)]
fn read_u64(buf: &[u8]) -> u64 {
    u64::from_be_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]])
}

#[inline(always)]
fn read_u48(buf: &[u8]) -> u64 {
    let b = buf;
    ((b[0] as u64) << 40) | ((b[1] as u64) << 32) | ((b[2] as u64) << 24)
        | ((b[3] as u64) << 16) | ((b[4] as u64) << 8) | (b[5] as u64)
}

#[inline(always)]
fn read_stock(buf: &[u8]) -> [u8; 8] {
    let mut s = [b' '; 8];
    s.copy_from_slice(&buf[..8]);
    s
}

/// Parse a single ITCH 5.0 message — fully inlined, unchecked reads.
pub fn parse_one(buf: &[u8]) -> Option<(Message, usize)> {
    if buf.is_empty() {
        return None;
    }

    let msg_type = buf[0];

    match msg_type {
        b'A' => {
            if buf.len() < 36 { return None; }
            let msg = super::naive::AddOrder {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                order_ref: read_u64(unsafe { buf.get_unchecked(11..19) }),
                buy: *unsafe { buf.get_unchecked(19) } == b'B',
                shares: read_u32(unsafe { buf.get_unchecked(20..24) }),
                stock: read_stock(unsafe { buf.get_unchecked(24..32) }),
                price: read_u32(unsafe { buf.get_unchecked(32..36) }),
            };
            Some((Message::AddOrder(msg), 36))
        }
        b'F' => {
            if buf.len() < 40 { return None; }
            let mut attr = [b' '; 4];
            attr.copy_from_slice(unsafe { buf.get_unchecked(36..40) });
            let msg = super::naive::AddOrderMpid {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                order_ref: read_u64(unsafe { buf.get_unchecked(11..19) }),
                buy: *unsafe { buf.get_unchecked(19) } == b'B',
                shares: read_u32(unsafe { buf.get_unchecked(20..24) }),
                stock: read_stock(unsafe { buf.get_unchecked(24..32) }),
                price: read_u32(unsafe { buf.get_unchecked(32..36) }),
                attribution: attr,
            };
            Some((Message::AddOrderMpid(msg), 40))
        }
        b'E' => {
            if buf.len() < 31 { return None; }
            let msg = super::naive::OrderExecuted {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                order_ref: read_u64(unsafe { buf.get_unchecked(11..19) }),
                executed_shares: read_u32(unsafe { buf.get_unchecked(19..23) }),
                match_number: read_u64(unsafe { buf.get_unchecked(23..31) }),
            };
            Some((Message::OrderExecuted(msg), 31))
        }
        b'C' => {
            if buf.len() < 36 { return None; }
            let msg = super::naive::OrderExecutedWithPrice {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                order_ref: read_u64(unsafe { buf.get_unchecked(11..19) }),
                executed_shares: read_u32(unsafe { buf.get_unchecked(19..23) }),
                match_number: read_u64(unsafe { buf.get_unchecked(23..31) }),
                printable: *unsafe { buf.get_unchecked(31) } == b'Y',
                execution_price: read_u32(unsafe { buf.get_unchecked(32..36) }),
            };
            Some((Message::OrderExecutedWithPrice(msg), 36))
        }
        b'X' => {
            if buf.len() < 23 { return None; }
            let msg = super::naive::OrderCancel {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                order_ref: read_u64(unsafe { buf.get_unchecked(11..19) }),
                canceled_shares: read_u32(unsafe { buf.get_unchecked(19..23) }),
            };
            Some((Message::OrderCancel(msg), 23))
        }
        b'D' => {
            if buf.len() < 19 { return None; }
            let msg = super::naive::OrderDelete {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                order_ref: read_u64(unsafe { buf.get_unchecked(11..19) }),
            };
            Some((Message::OrderDelete(msg), 19))
        }
        b'S' => {
            if buf.len() < 12 { return None; }
            let msg = super::naive::SystemEvent {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                event_code: *unsafe { buf.get_unchecked(11) },
            };
            Some((Message::SystemEvent(msg), 12))
        }
        b'L' => {
            if buf.len() < 26 { return None; }
            let mut mpid = [b' '; 4];
            mpid.copy_from_slice(unsafe { buf.get_unchecked(11..15) });
            let msg = super::naive::MarketParticipantPosition {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                mpid,
                stock: read_stock(unsafe { buf.get_unchecked(15..23) }),
                primary_market_maker: *unsafe { buf.get_unchecked(23) } == b'Y',
                market_maker_mode: *unsafe { buf.get_unchecked(24) },
                market_participant_state: *unsafe { buf.get_unchecked(25) },
            };
            Some((Message::MarketParticipantPosition(msg), 26))
        }
        b'P' => {
            if buf.len() < 44 { return None; }
            let msg = super::naive::Trade {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                order_ref: read_u64(unsafe { buf.get_unchecked(11..19) }),
                buy: *unsafe { buf.get_unchecked(19) } == b'B',
                shares: read_u32(unsafe { buf.get_unchecked(20..24) }),
                stock: read_stock(unsafe { buf.get_unchecked(24..32) }),
                price: read_u32(unsafe { buf.get_unchecked(32..36) }),
                match_number: read_u64(unsafe { buf.get_unchecked(36..44) }),
            };
            Some((Message::Trade(msg), 44))
        }
        b'Q' => {
            if buf.len() < 40 { return None; }
            let msg = super::naive::CrossTrade {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                shares: read_u64(unsafe { buf.get_unchecked(11..19) }),
                stock: read_stock(unsafe { buf.get_unchecked(19..27) }),
                cross_price: read_u32(unsafe { buf.get_unchecked(27..31) }),
                match_number: read_u64(unsafe { buf.get_unchecked(31..39) }),
                cross_type: *unsafe { buf.get_unchecked(39) },
            };
            Some((Message::CrossTrade(msg), 40))
        }
        b'B' => {
            if buf.len() < 19 { return None; }
            let msg = super::naive::BrokenTrade {
                stock_locate: read_u16(unsafe { buf.get_unchecked(1..3) }),
                tracking_number: read_u16(unsafe { buf.get_unchecked(3..5) }),
                timestamp_ns: read_u48(unsafe { buf.get_unchecked(5..11) }),
                match_number: read_u64(unsafe { buf.get_unchecked(11..19) }),
            };
            Some((Message::BrokenTrade(msg), 19))
        }
        _ => Some((Message::Unknown { msg_type }, 1)),
    }
}

/// Parse all messages — pre-allocated output, tight loop.
pub fn parse_all(buf: &[u8]) -> Vec<Message> {
    let estimated_count = buf.len() / 24;
    let mut msgs = Vec::with_capacity(estimated_count);
    let mut pos = 0;
    let len = buf.len();

    while pos + 2 <= len {
        let msg_len = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
        let msg_start = pos + 2;
        let msg_end = msg_start + msg_len;
        if msg_end > len { break; }
        if let Some((msg, _)) = parse_one(&buf[msg_start..msg_end]) {
            msgs.push(msg);
        }
        pos = msg_end;
    }
    msgs
}
