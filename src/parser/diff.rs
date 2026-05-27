//! Differential testing framework — optimized parser must match naive parser field-by-field.
//! This is the correctness gate: no optimization ships without passing this.

#[cfg(test)]
mod tests {
    use crate::parser::naive;
    use crate::parser::optimized;
    use crate::data::gen;
    use std::hint::black_box;

    fn build_test_stream() -> Vec<u8> {
        let mut msgs: Vec<Vec<u8>> = Vec::new();

        for code in [b'O', b'S', b'Q'] {
            msgs.push(gen::build_system_event(code));
        }
        for i in 0u64..50 {
            msgs.push(gen::build_add_order(i, i % 2 == 0, 100 + (i % 50) as u32, 1000000 + (i % 1000) as u32));
        }
        for i in 0u64..20 {
            msgs.push(gen::build_order_executed(i, 50, i * 10));
        }
        for i in 0u64..10 {
            msgs.push(gen::build_order_cancel(i, 25));
        }
        for i in 0u64..10 {
            msgs.push(gen::build_order_delete(i));
        }
        for i in 0u64..10 {
            msgs.push(gen::build_trade(i, 100, 1000000 + (i % 100) as u32, i * 100));
        }

        let mut buf = Vec::new();
        for msg in &msgs {
            let len = msg.len() as u16;
            buf.extend_from_slice(&len.to_be_bytes());
            buf.extend_from_slice(msg);
        }
        buf
    }

    #[test]
    fn differential_parse_all() {
        let data = build_test_stream();
        let data = black_box(&data);

        let naive_msgs = naive::parse_all(data);
        let opt_msgs = optimized::parse_all(data);

        assert_eq!(naive_msgs.len(), opt_msgs.len(),
            "message count mismatch: naive={} opt={}", naive_msgs.len(), opt_msgs.len());

        for (i, (n, o)) in naive_msgs.iter().zip(opt_msgs.iter()).enumerate() {
            assert_eq!(n, o, "message {i} mismatch:\n  naive: {n:?}\n  opt:   {o:?}");
        }
    }

    #[test]
    fn differential_parse_one() {
        let add_msg = gen::build_add_order(42, true, 100, 50000);
        let (naive_msg, naive_len) = naive::parse_one(&add_msg).unwrap();
        let (opt_msg, opt_len) = optimized::parse_one(&add_msg).unwrap();
        assert_eq!(naive_len, opt_len);
        assert_eq!(naive_msg, opt_msg);

        let del_msg = gen::build_order_delete(42);
        let (naive_msg, _) = naive::parse_one(&del_msg).unwrap();
        let (opt_msg, _) = optimized::parse_one(&del_msg).unwrap();
        assert_eq!(naive_msg, opt_msg);

        let sys_msg = gen::build_system_event(b'O');
        let (naive_msg, _) = naive::parse_one(&sys_msg).unwrap();
        let (opt_msg, _) = optimized::parse_one(&sys_msg).unwrap();
        assert_eq!(naive_msg, opt_msg);
    }

    #[test]
    fn differential_with_shuffled_data() {
        let (natural, shuffled) = gen::generate_paired_streams(1000, 500, 200);
        for (label, data) in &[("natural", &natural), ("shuffled", &shuffled)] {
            let naive_msgs = naive::parse_all(data);
            let opt_msgs = optimized::parse_all(data);
            assert_eq!(naive_msgs.len(), opt_msgs.len(), "[{label}] count mismatch");
            for (i, (n, o)) in naive_msgs.iter().zip(opt_msgs.iter()).enumerate() {
                assert_eq!(n, o, "[{label}] msg {i} mismatch");
            }
        }
    }

    #[test]
    fn differential_all_message_types() {
        let stream = gen::generate_full_stream(50);
        let naive_msgs = naive::parse_all(&stream);
        let opt_msgs = optimized::parse_all(&stream);

        assert_eq!(naive_msgs.len(), opt_msgs.len());
        for (i, (n, o)) in naive_msgs.iter().zip(opt_msgs.iter()).enumerate() {
            assert_eq!(n, o, "full stream msg {i} mismatch");
        }

        let mut has_system = false;
        let mut has_add = false;
        let mut has_exec = false;
        let mut has_cancel = false;
        let mut has_delete = false;
        let mut has_trade = false;
        for msg in &naive_msgs {
            match msg {
                naive::Message::SystemEvent(_) => has_system = true,
                naive::Message::AddOrder(_) => has_add = true,
                naive::Message::OrderExecuted(_) => has_exec = true,
                naive::Message::OrderCancel(_) => has_cancel = true,
                naive::Message::OrderDelete(_) => has_delete = true,
                naive::Message::Trade(_) => has_trade = true,
                _ => {}
            }
        }
        assert!(has_system && has_add && has_exec && has_cancel && has_delete && has_trade);
    }

    #[test]
    fn boundary_empty_input() {
        assert_eq!(naive::parse_all(&[]).len(), 0);
        assert_eq!(optimized::parse_all(&[]).len(), 0);
        assert!(naive::parse_one(&[]).is_none());
        assert!(optimized::parse_one(&[]).is_none());
    }

    #[test]
    fn boundary_truncated_length_prefix() {
        // Only 1 byte of length prefix
        assert_eq!(naive::parse_all(&[0x00]).len(), 0);
        assert_eq!(optimized::parse_all(&[0x00]).len(), 0);
    }

    #[test]
    fn boundary_truncated_message_body() {
        // Length says 36 bytes but only 5 available
        let mut buf = Vec::new();
        buf.extend_from_slice(&36u16.to_be_bytes());
        buf.extend_from_slice(&[b'A', 0, 0, 0]); // too short
        assert_eq!(naive::parse_all(&buf).len(), 0);
        assert_eq!(optimized::parse_all(&buf).len(), 0);
    }

    #[test]
    fn unknown_message_type_handling() {
        let msg = vec![b'Z', 1, 2, 3]; // Unknown type + some garbage
        let (naive_msg, _) = naive::parse_one(&msg).unwrap();
        let (opt_msg, _) = optimized::parse_one(&msg).unwrap();
        assert_eq!(naive_msg, opt_msg);
        match naive_msg {
            naive::Message::Unknown { msg_type } => assert_eq!(msg_type, b'Z'),
            _ => panic!("expected Unknown"),
        }
    }

    #[test]
    fn fuzz_random_bytes() {
        // Pseudo-random fuzz: generate random byte sequences and ensure both parsers
        // produce identical results (both should handle garbage gracefully)
        let seed = 12345u64;
        let mut state = seed;
        let mut rng = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            state
        };

        for _ in 0..100 {
            let len = ((rng() % 200) as usize) + 2;
            let mut buf = Vec::with_capacity(len);
            for _ in 0..len {
                buf.push((rng() & 0xFF) as u8);
            }
            let naive_msgs = naive::parse_all(&buf);
            let opt_msgs = optimized::parse_all(&buf);
            assert_eq!(naive_msgs.len(), opt_msgs.len());
            for (i, (n, o)) in naive_msgs.iter().zip(opt_msgs.iter()).enumerate() {
                assert_eq!(n, o, "fuzz mismatch at msg {i}");
            }
        }
    }

    #[test]
    fn edge_case_max_values() {
        // Add Order with maximum field values
        let mut msg = Vec::new();
        msg.push(b'A');
        msg.extend_from_slice(&u16::MAX.to_be_bytes());
        msg.extend_from_slice(&u16::MAX.to_be_bytes());
        msg.extend_from_slice(&u64::MAX.to_be_bytes()[2..8]); // 6-byte timestamp
        msg.extend_from_slice(&u64::MAX.to_be_bytes());
        msg.push(b'B');
        msg.extend_from_slice(&u32::MAX.to_be_bytes());
        msg.extend_from_slice(b"MAXVAL  ");
        msg.extend_from_slice(&u32::MAX.to_be_bytes());

        let (naive_msg, naive_len) = naive::parse_one(&msg).unwrap();
        let (opt_msg, opt_len) = optimized::parse_one(&msg).unwrap();
        assert_eq!(naive_len, opt_len);
        assert_eq!(naive_msg, opt_msg);

        match naive_msg {
            naive::Message::AddOrder(a) => {
                assert_eq!(a.stock_locate, u16::MAX);
                assert_eq!(a.order_ref, u64::MAX);
                assert_eq!(a.shares, u32::MAX);
                assert_eq!(a.price, u32::MAX);
            }
            _ => panic!("expected AddOrder"),
        }
    }
}
