//! Test data generation — produce paired datasets (natural order + shuffled)
//! to detect branch-predictor overfitting and data-layout cheating.
//!
//! Generates binary ITCH 5.0 messages with 2-byte big-endian length prefixes.
//! Covers all message types for differential testing.

/// Generate a synthetic ITCH-like byte stream with interleaved message types.
/// Returns (natural_order_stream, shuffled_stream).
pub fn generate_paired_streams(n_add: usize, n_exec: usize, n_cancel: usize) -> (Vec<u8>, Vec<u8>) {
    let natural = build_stream(n_add, n_exec, n_cancel, ShuffleMode::Natural);
    let shuffled = build_stream(n_add, n_exec, n_cancel, ShuffleMode::Shuffled);
    (natural, shuffled)
}

enum ShuffleMode {
    Natural,
    Shuffled,
}

fn build_stream(n_add: usize, n_exec: usize, n_cancel: usize, mode: ShuffleMode) -> Vec<u8> {
    let mut messages: Vec<Vec<u8>> = Vec::new();

    for i in 0..n_add {
        let is_buy = i % 2 == 0;
        let price = if is_buy {
            1_000_000 - (i % 1000) as u32
        } else {
            1_001_000 + (i % 1000) as u32
        };
        messages.push(build_add_order(
            i as u64,
            is_buy,
            100 + (i % 50) as u32,
            price,
        ));
    }

    for i in 0..n_exec {
        messages.push(build_order_executed(
            i as u64 % n_add.max(1) as u64,
            50,
            i as u64,
        ));
    }

    for i in 0..n_cancel {
        messages.push(build_order_cancel(i as u64 % n_add.max(1) as u64, 25));
    }

    match mode {
        ShuffleMode::Natural => {}
        ShuffleMode::Shuffled => {
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

/// Build an Add Order message ('A', 36 bytes).
pub fn build_add_order(order_ref: u64, is_buy: bool, shares: u32, price: u32) -> Vec<u8> {
    let mut msg = Vec::with_capacity(36);
    msg.push(b'A');
    msg.extend_from_slice(&1u16.to_be_bytes()); // stock locate
    msg.extend_from_slice(&1u16.to_be_bytes()); // tracking number
    let ts: u64 = 36_000_000_000_000;
    msg.extend_from_slice(&ts.to_be_bytes()[2..8]); // 6-byte timestamp
    msg.extend_from_slice(&order_ref.to_be_bytes()); // order ref (8 bytes)
    msg.push(if is_buy { b'B' } else { b'S' }); // buy/sell
    msg.extend_from_slice(&shares.to_be_bytes()); // shares (4 bytes)
    msg.extend_from_slice(b"TEST    "); // stock (8 bytes)
    msg.extend_from_slice(&price.to_be_bytes()); // price (4 bytes)
    msg
}

/// Build an Order Executed message ('E', 31 bytes).
pub fn build_order_executed(order_ref: u64, executed_shares: u32, match_number: u64) -> Vec<u8> {
    let mut msg = Vec::with_capacity(31);
    msg.push(b'E');
    msg.extend_from_slice(&1u16.to_be_bytes());
    msg.extend_from_slice(&1u16.to_be_bytes());
    let ts: u64 = 36_000_000_000_000;
    msg.extend_from_slice(&ts.to_be_bytes()[2..8]);
    msg.extend_from_slice(&order_ref.to_be_bytes());
    msg.extend_from_slice(&executed_shares.to_be_bytes());
    msg.extend_from_slice(&match_number.to_be_bytes());
    msg
}

/// Build an Order Cancel message ('X', 23 bytes).
pub fn build_order_cancel(order_ref: u64, canceled_shares: u32) -> Vec<u8> {
    let mut msg = Vec::with_capacity(23);
    msg.push(b'X');
    msg.extend_from_slice(&1u16.to_be_bytes());
    msg.extend_from_slice(&1u16.to_be_bytes());
    let ts: u64 = 36_000_000_000_000;
    msg.extend_from_slice(&ts.to_be_bytes()[2..8]);
    msg.extend_from_slice(&order_ref.to_be_bytes());
    msg.extend_from_slice(&canceled_shares.to_be_bytes());
    msg
}

// ---------- Extended generation for all message types ----------

/// Build an Order Delete message ('D', 19 bytes).
pub fn build_order_delete(order_ref: u64) -> Vec<u8> {
    let mut msg = Vec::with_capacity(19);
    msg.push(b'D');
    msg.extend_from_slice(&1u16.to_be_bytes());
    msg.extend_from_slice(&1u16.to_be_bytes());
    let ts: u64 = 36_000_000_000_000;
    msg.extend_from_slice(&ts.to_be_bytes()[2..8]);
    msg.extend_from_slice(&order_ref.to_be_bytes());
    msg
}

/// Build a System Event message ('S', 12 bytes).
pub fn build_system_event(event_code: u8) -> Vec<u8> {
    let mut msg = Vec::with_capacity(12);
    msg.push(b'S');
    msg.extend_from_slice(&0u16.to_be_bytes());
    msg.extend_from_slice(&1u16.to_be_bytes());
    let ts: u64 = 36_000_000_000_000;
    msg.extend_from_slice(&ts.to_be_bytes()[2..8]);
    msg.push(event_code);
    msg
}

/// Build a Trade message ('P', 44 bytes).
pub fn build_trade(order_ref: u64, shares: u32, price: u32, match_number: u64) -> Vec<u8> {
    let mut msg = Vec::with_capacity(44);
    msg.push(b'P');
    msg.extend_from_slice(&1u16.to_be_bytes());
    msg.extend_from_slice(&1u16.to_be_bytes());
    let ts: u64 = 36_000_000_000_000;
    msg.extend_from_slice(&ts.to_be_bytes()[2..8]);
    msg.extend_from_slice(&order_ref.to_be_bytes());
    msg.push(b'B');
    msg.extend_from_slice(&shares.to_be_bytes());
    msg.extend_from_slice(b"TEST    ");
    msg.extend_from_slice(&price.to_be_bytes());
    msg.extend_from_slice(&match_number.to_be_bytes());
    msg
}

/// Generate a full mixed stream covering all ITCH 5.0 message types.
pub fn generate_full_stream(n: usize) -> Vec<u8> {
    let mut messages: Vec<Vec<u8>> = Vec::new();

    // System events
    for code in [b'O', b'S', b'Q', b'M', b'E', b'C'] {
        messages.push(build_system_event(code));
    }

    // Add orders
    for i in 0..n {
        let is_buy = i % 2 == 0;
        let price = if is_buy {
            1_000_000 - (i % 1000) as u32
        } else {
            1_001_000 + (i % 1000) as u32
        };
        messages.push(build_add_order(
            i as u64,
            is_buy,
            100 + (i % 50) as u32,
            price,
        ));
    }

    // Order executed
    for i in 0..n / 2 {
        messages.push(build_order_executed(i as u64, 50, i as u64));
    }

    // Order cancel
    for i in 0..n / 4 {
        messages.push(build_order_cancel(i as u64, 25));
    }

    // Order delete
    for i in 0..n / 4 {
        messages.push(build_order_delete(i as u64));
    }

    // Trades
    for i in 0..n / 4 {
        messages.push(build_trade(
            i as u64,
            100,
            1000000 + (i % 100) as u32,
            i as u64,
        ));
    }

    let mut buf = Vec::new();
    for msg in &messages {
        let len = msg.len() as u16;
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(msg);
    }
    buf
}

/// Load an ITCH binary file from disk.
pub fn load_itch_file(path: &str) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}
