//! Test data generation — produce paired datasets (natural order + shuffled)
//! to detect branch-predictor overfitting and data-layout cheating.

/// Generate a synthetic ITCH-like byte stream with interleaved message types.
/// Returns (natural_order_stream, shuffled_stream).
pub fn generate_paired_streams(n_add: usize, n_exec: usize, n_cancel: usize) -> (Vec<u8>, Vec<u8>) {
    let natural = build_stream(n_add, n_exec, n_cancel, ShuffleMode::Natural);
    let shuffled = build_stream(n_add, n_exec, n_cancel, ShuffleMode::Shuffled);
    (natural, shuffled)
}

enum ShuffleMode {
    Natural,  // all adds, then all executes, then all cancels
    Shuffled, // random interleaving
}

fn build_stream(n_add: usize, n_exec: usize, n_cancel: usize, mode: ShuffleMode) -> Vec<u8> {
    let mut messages: Vec<Vec<u8>> = Vec::new();

    for i in 0..n_add {
        let mut msg = Vec::new();
        msg.push(b'A');
        msg.extend_from_slice(b"01");
        msg.extend_from_slice(b"01");
        msg.extend_from_slice(b"00000000123");
        msg.extend_from_slice(&format!("{:08}", i).as_bytes());
        msg.push(if i % 2 == 0 { b'B' } else { b'S' });
        msg.extend_from_slice(b"000100");
        msg.extend_from_slice(b"TEST    ");
        msg.extend_from_slice(b"0000010000");
        messages.push(msg);
    }

    for i in 0..n_exec {
        let mut msg = Vec::new();
        msg.push(b'E');
        msg.extend_from_slice(b"01");
        msg.extend_from_slice(b"01");
        msg.extend_from_slice(b"00000000123");
        msg.extend_from_slice(&format!("{:08}", i % n_add.max(1)).as_bytes());
        msg.extend_from_slice(b"000050");
        msg.extend_from_slice(&format!("{:08}", i).as_bytes());
        messages.push(msg);
    }

    for i in 0..n_cancel {
        let mut msg = Vec::new();
        msg.push(b'X');
        msg.extend_from_slice(b"01");
        msg.extend_from_slice(b"01");
        msg.extend_from_slice(b"00000000123");
        msg.extend_from_slice(&format!("{:08}", i % n_add.max(1)).as_bytes());
        msg.extend_from_slice(b"000025");
        messages.push(msg);
    }

    match mode {
        ShuffleMode::Natural => {} // keep as-is
        ShuffleMode::Shuffled => {
            // Simple Fisher-Yates shuffle
            use std::time::SystemTime;
            let seed = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            let mut state = seed;
            for i in (1..messages.len()).rev() {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let j = (state >> 33) as usize % (i + 1);
                messages.swap(i, j);
            }
        }
    }

    let mut buf = Vec::new();
    for msg in &messages {
        let len = msg.len() as u16;
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(msg);
    }
    buf
}
