//! io_uring benchmark harness — measures kernel-bypass overhead.
//!
//! Since io_uring requires the `io_uring` crate and kernel 5.1+,
//! this provides a simulation mode for environments without io_uring.
//! Real implementation would use:
//!   - io_uring::IoUring for async I/O
//!   - SQPOLL mode for kernel-side polling
//!   - Fixed buffers for zero-copy

use crate::data::gen;
use crate::histogram::LatencyReport;
use crate::latency_buf::LatencyBuffer;
use crate::parser;
use crate::timer;

/// Simulated io_uring submission/completion cycle.
/// Measures the overhead of a "submit → wait → process" round trip.
/// Simulated io_uring submission/completion cycle.
/// Measures the overhead of a "submit → wait → process" round trip.
///
/// HONEST DISCLAIMER: This simulation does NOT model SQPOLL amortization.
/// Real io_uring's value proposition is that batch submission with SQPOLL mode
/// does NOT increase syscall count — the kernel polls the SQ ring directly.
/// Here each "batch" is just the loop body repeated, so linear scaling with
/// batch size is expected and NOT representative of real io_uring behavior.
/// The p50 numbers below are meaningless for predicting real io_uring performance.
pub fn io_uring_simulated_bench(iters: usize, ghz: f64) {
    // Simulate the io_uring submission queue + completion queue cycle
    // by measuring round-trip latency of a simulated "submit + reap" cycle

    let batch_sizes = [1, 4, 16, 64, 256];

    for &batch_size in &batch_sizes {
        let mut buf = LatencyBuffer::with_capacity(iters / batch_size + 1);
        let n_batches = iters / batch_size;

        // Prepare data
        let (stream, _) = gen::generate_paired_streams(batch_size, batch_size / 2, batch_size / 4);

        for _ in 0..n_batches {
            // Simulate: submit batch → kernel processes → completion arrives
            let submit_start = timer::rdtsc_serialized();

            // Simulate SQE submission (memcpy to SQ ring buffer)
            let _sqes: Vec<&[u8]> = vec![&stream];

            // Simulate kernel processing: parse the messages
            let msgs = parser::optimized::parse_all(&stream);

            // Simulate CQE reap
            let _cqe_count = msgs.len();

            let completion_end = timer::rdtsc_serialized();
            buf.record(completion_end - submit_start);
        }

        let report = LatencyReport::from_cycles(buf.finish(), ghz);
        report.print(&format!("io_uring_sim_batch_{}", batch_size));
    }

    println!("\nio_uring Simulation Notes:");
    println!("DISCLAIMER: This simulation does NOT model SQPOLL amortization.");
    println!("The linear scaling with batch size is expected — each 'batch' is just");
    println!("the same loop body repeated. Real io_uring with SQPOLL would show");
    println!("near-constant latency regardless of batch size (no per-entry syscall).");
    println!("- Real io_uring would use SQPOLL for kernel-side polling");
    println!("- Fixed buffers enable zero-copy (no memcpy to SQ)");
    println!("- For HFT: batch=1 with SQPOLL gives ~600ns kernel bypass on Zen 3");
}

/// Measure syscall overhead (baseline for kernel bypass comparison).
pub fn syscall_overhead_bench(ghz: f64) {
    let n = 10_000;
    let mut buf = LatencyBuffer::with_capacity(n);

    for _ in 0..n {
        let start = timer::rdtsc_serialized();
        // sched_yield is a lightweight syscall — measures minimum kernel round-trip
        std::thread::yield_now();
        let elapsed = timer::rdtsc_serialized() - start;
        buf.record(elapsed);
    }

    let report = LatencyReport::from_cycles(buf.finish(), ghz);
    report.print("syscall-yield-overhead");

    let mut buf_read = LatencyBuffer::with_capacity(n);
    for _ in 0..n {
        let start = timer::rdtsc_serialized();
        // Read from /proc/self/status — measures file I/O syscall overhead
        let _ = std::fs::read_to_string("/proc/self/status");
        let elapsed = timer::rdtsc_serialized() - start;
        buf_read.record(elapsed);
    }

    let report_read = LatencyReport::from_cycles(buf_read.finish(), ghz);
    report_read.print("syscall-read-proc-overhead");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer;

    #[test]
    fn io_uring_sim_runs() {
        let ghz = timer::calibrate_ghz();
        io_uring_simulated_bench(100, ghz);
    }
}
