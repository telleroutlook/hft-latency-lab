//! Differential testing framework — optimized parser must match naive parser field-by-field.
//! This is the correctness gate: no optimization ships without passing this.

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    /// Generate a synthetic ITCH message stream for testing.
    /// TODO: replace with real NASDAQ TotalView-ITCH sample data.
    fn synthetic_itch_stream() -> Vec<u8> {
        let mut buf = Vec::new();
        // Build a few Add Order messages
        for i in 0u64..100 {
            let mut msg = Vec::new();
            msg.push(b'A');                        // msg type
            msg.extend_from_slice(b"01");           // stock locate
            msg.extend_from_slice(b"01");           // tracking number
            msg.extend_from_slice(b"00000000123");  // timestamp (12 bytes)
            msg.extend_from_slice(&format!("{:08}", i).as_bytes()); // order ref
            msg.push(b'B');                         // buy/sell
            msg.extend_from_slice(b"000100");       // shares
            msg.extend_from_slice(b"STOCK   ");     // stock (8 bytes)
            msg.extend_from_slice(b"0000010000");   // price (10 bytes)

            // Prepend 2-byte length
            let len = msg.len() as u16;
            buf.extend_from_slice(&len.to_be_bytes());
            buf.extend_from_slice(&msg);
        }
        buf
    }

    #[test]
    fn differential_parse_all() {
        let data = synthetic_itch_stream();
        let data = black_box(&data);

        let naive_msgs = naive::parse_all(data);
        let opt_msgs = optimized::parse_all(data);

        assert_eq!(naive_msgs.len(), opt_msgs.len(),
            "message count mismatch: naive={} opt={}",
            naive_msgs.len(), opt_msgs.len());

        for (i, (n, o)) in naive_msgs.iter().zip(opt_msgs.iter()).enumerate() {
            assert_eq!(n, o, "message {i} mismatch:\n  naive: {n:?}\n  opt:   {o:?}");
        }
    }

    #[test]
    fn differential_parse_one() {
        let stream = synthetic_itch_stream();
        // Skip 2-byte length prefix
        let data = &stream[2..];

        let (naive_msg, naive_len) = naive::parse_one(data).expect("naive should parse");
        let (opt_msg, opt_len) = optimized::parse_one(data).expect("opt should parse");

        assert_eq!(naive_len, opt_len, "consumed bytes differ");
        assert_eq!(naive_msg, opt_msg, "parsed message differs");
    }
}
