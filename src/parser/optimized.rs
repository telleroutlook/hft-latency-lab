//! Optimized ITCH parser — to be optimized iteratively with measurement discipline.
//! Start as a copy of naive, then apply one optimization at a time, each backed by perf evidence.

use super::naive::Message;

/// Parse a single ITCH message. Initially identical to naive — optimizations go here.
/// Every optimization must:
///   1. Be preceded by perf counter evidence
///   2. Pass differential test against naive
///   3. Show p99.9 improvement in the latency report
pub fn parse_one(buf: &[u8]) -> Option<(Message, usize)> {
    super::naive::parse_one(buf)
}

/// Parse all messages from a byte buffer.
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
