pub mod spsc;

use crate::histogram::LatencyReport;
use crate::latency_buf::LatencyBuffer;
use crate::orderbook::book::OrderBook;
use crate::parser;
use crate::parser::naive::Message;
use crate::timer;

/// Run the full pipeline: parse → SPSC → order book → BBO callback.
/// Returns per-stage latency samples for analysis.
pub struct PipelineResult {
    pub parse_report: LatencyReport,
    pub queue_report: LatencyReport,
    pub book_report: LatencyReport,
    pub e2e_report: LatencyReport,
    pub messages_processed: usize,
    pub bbo_changes: u64,
}

/// Run the pipeline with a given byte stream, measuring each stage.
pub fn run_pipeline(stream: &[u8], ghz: f64) -> PipelineResult {
    let msg_count_est = stream.len() / 24;
    let ring = spsc::SpscRing::<Message, 65536>::new();

    let mut parse_buf = LatencyBuffer::with_capacity(msg_count_est);
    let mut queue_buf = LatencyBuffer::with_capacity(msg_count_est);
    let mut book_buf = LatencyBuffer::with_capacity(msg_count_est);
    let mut e2e_buf = LatencyBuffer::with_capacity(msg_count_est);

    let bbo_count = 0u64;
    let mut book = OrderBook::new(msg_count_est);
    book.set_bbo_callback(Box::new(|_bid, _ask| {}));

    // Stage 1: Parse all messages
    let parse_start = timer::rdtsc_serialized();
    let msgs = parser::optimized::parse_all(stream);
    let parse_total = timer::rdtsc_serialized() - parse_start;
    parse_buf.record(parse_total);

    // Stage 2+3: Push through SPSC → consume → feed to order book
    for msg in &msgs {
        let e2e_start = timer::rdtsc_serialized();

        // Push to queue
        let q_start = timer::rdtsc_serialized();
        while !ring.push(msg.clone()) {}
        let q_elapsed = timer::rdtsc_serialized() - q_start;
        queue_buf.record(q_elapsed);

        // Pop from queue
        let popped = ring.pop().expect("queue should have item");

        // Feed to order book
        let b_start = timer::rdtsc_serialized();
        match &popped {
            Message::AddOrder(a) => {
                book.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
            }
            Message::OrderCancel(c) => {
                book.cancel_order(c.order_ref);
            }
            Message::OrderDelete(d) => {
                book.delete_order(d.order_ref);
            }
            Message::OrderExecuted(e) => {
                book.execute_order(e.order_ref, e.executed_shares);
            }
            _ => {}
        }
        let b_elapsed = timer::rdtsc_serialized() - b_start;
        book_buf.record(b_elapsed);

        let e2e_elapsed = timer::rdtsc_serialized() - e2e_start;
        e2e_buf.record(e2e_elapsed);
    }

    let parse_report = LatencyReport::from_cycles(parse_buf.finish(), ghz);
    let queue_report = LatencyReport::from_cycles(queue_buf.finish(), ghz);
    let book_report = LatencyReport::from_cycles(book_buf.finish(), ghz);
    let e2e_report = LatencyReport::from_cycles(e2e_buf.finish(), ghz);

    PipelineResult {
        parse_report,
        queue_report,
        book_report,
        e2e_report,
        messages_processed: msgs.len(),
        bbo_changes: bbo_count,
    }
}
