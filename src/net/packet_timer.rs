//! Packet-level timing — measure latency from packet arrival to strategy callback.
//! Uses TSC timestamps for nanosecond precision.

use crate::timer;

/// Timestamps for a single packet's journey through the pipeline.
#[derive(Debug, Clone)]
pub struct PacketTimestamps {
    /// TSC when packet was "received" (or injected for benchmarking)
    pub arrival: u64,
    /// TSC when parsing started
    pub parse_start: u64,
    /// TSC when parsing finished
    pub parse_end: u64,
    /// TSC when order book update finished
    pub book_update_end: u64,
    /// TSC when strategy callback fired
    pub strategy_callback: u64,
}

impl PacketTimestamps {
    pub fn new() -> Self {
        let now = timer::rdtsc_serialized();
        Self {
            arrival: now,
            parse_start: now,
            parse_end: now,
            book_update_end: now,
            strategy_callback: now,
        }
    }

    pub fn total_latency_cycles(&self) -> u64 {
        self.strategy_callback - self.arrival
    }

    pub fn parse_latency_cycles(&self) -> u64 {
        self.parse_end - self.parse_start
    }

    pub fn book_latency_cycles(&self) -> u64 {
        self.book_update_end - self.parse_end
    }

    pub fn strategy_latency_cycles(&self) -> u64 {
        self.strategy_callback - self.book_update_end
    }

    /// Convert all latencies to nanoseconds.
    pub fn to_ns(&self, ghz: f64) -> PacketLatencyNs {
        PacketLatencyNs {
            total_ns: (self.total_latency_cycles() as f64 / ghz) as u64,
            parse_ns: (self.parse_latency_cycles() as f64 / ghz) as u64,
            book_ns: (self.book_latency_cycles() as f64 / ghz) as u64,
            strategy_ns: (self.strategy_latency_cycles() as f64 / ghz) as u64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PacketLatencyNs {
    pub total_ns: u64,
    pub parse_ns: u64,
    pub book_ns: u64,
    pub strategy_ns: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_timestamps_monotonic() {
        let mut ts = PacketTimestamps::new();
        ts.parse_start = timer::rdtsc_serialized();
        ts.parse_end = timer::rdtsc_serialized();
        ts.book_update_end = timer::rdtsc_serialized();
        ts.strategy_callback = timer::rdtsc_serialized();

        assert!(ts.arrival <= ts.parse_start);
        assert!(ts.parse_start <= ts.parse_end);
        assert!(ts.parse_end <= ts.book_update_end);
        assert!(ts.book_update_end <= ts.strategy_callback);
        assert!(ts.total_latency_cycles() > 0);
    }
}
