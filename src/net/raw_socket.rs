//! Raw socket interface for packet capture simulation.
//! In a real deployment, this would bind to a network interface.
//! For benchmarking, we inject synthetic ITCH packets.

use crate::parser;
use crate::orderbook::book::OrderBook;
use crate::data::gen;
use crate::timer;
use crate::latency_buf::LatencyBuffer;
use crate::histogram::LatencyReport;
use crate::net::packet_timer::PacketTimestamps;

/// Simulated packet receiver — injects ITCH messages and measures end-to-end latency.
pub struct PacketReceiver {
    book: OrderBook,
    timestamps: Vec<PacketTimestamps>,
}

impl PacketReceiver {
    pub fn new(book_capacity: usize) -> Self {
        Self {
            book: OrderBook::new(book_capacity),
            timestamps: Vec::new(),
        }
    }

    /// Process a batch of ITCH messages, recording per-message timestamps.
    pub fn process_stream(&mut self, stream: &[u8]) -> &Vec<PacketTimestamps> {
        // Parse all messages first (simulating batch reception)
        let msgs = parser::optimized::parse_all(stream);

        self.timestamps.clear();
        self.timestamps.reserve(msgs.len());

        for msg in &msgs {
            let mut ts = PacketTimestamps::new();
            ts.parse_start = timer::rdtsc_serialized();

            // "Parse" already done, but simulate per-message parse timing
            ts.parse_end = timer::rdtsc_serialized();

            // Feed to order book
            match msg {
                parser::naive::Message::AddOrder(a) => {
                    self.book.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
                }
                parser::naive::Message::OrderCancel(c) => {
                    self.book.cancel_order(c.order_ref);
                }
                parser::naive::Message::OrderDelete(d) => {
                    self.book.delete_order(d.order_ref);
                }
                parser::naive::Message::OrderExecuted(e) => {
                    self.book.execute_order(e.order_ref, e.executed_shares);
                }
                _ => {}
            }
            ts.book_update_end = timer::rdtsc_serialized();

            // Strategy callback (minimal)
            ts.strategy_callback = timer::rdtsc_serialized();

            self.timestamps.push(ts);
        }

        &self.timestamps
    }

    /// Generate a latency report from recorded timestamps.
    pub fn latency_report(&self, ghz: f64) -> (LatencyReport, LatencyReport, LatencyReport) {
        let total_cycles: Vec<u64> = self.timestamps.iter()
            .map(|ts| ts.total_latency_cycles())
            .collect();
        let book_cycles: Vec<u64> = self.timestamps.iter()
            .map(|ts| ts.book_latency_cycles())
            .collect();
        let strategy_cycles: Vec<u64> = self.timestamps.iter()
            .map(|ts| ts.strategy_latency_cycles())
            .collect();

        let total_report = LatencyReport::from_cycles(&total_cycles, ghz);
        let book_report = LatencyReport::from_cycles(&book_cycles, ghz);
        let strategy_report = LatencyReport::from_cycles(&strategy_cycles, ghz);

        (total_report, book_report, strategy_report)
    }

    pub fn book(&self) -> &OrderBook {
        &self.book
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer;

    #[test]
    fn packet_receiver_basic() {
        let ghz = timer::calibrate_ghz();
        let mut receiver = PacketReceiver::new(1000);

        let (stream, _) = gen::generate_paired_streams(100, 50, 20);
        let timestamps = receiver.process_stream(&stream);

        assert!(!timestamps.is_empty());
        for ts in timestamps {
            assert!(ts.total_latency_cycles() > 0);
        }

        let (total, book, _strategy) = receiver.latency_report(ghz);
        assert!(total.count() > 0);
        assert!(book.count() > 0);
    }
}
