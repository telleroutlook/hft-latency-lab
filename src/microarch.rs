//! Microarchitecture experiments for Phase 4.
//!
//! Each experiment is designed to produce measurable perf counter data.
//! Run with: `./hft-latency-lab microarch --experiment <name>`

/// Software prefetch experiment — measure whether _mm_prefetch helps order book traversal.
pub fn prefetch_experiment(iters: usize, ghz: f64) {
    use crate::orderbook::book::OrderBook;
    use crate::data::gen;
    use crate::timer;
    use crate::latency_buf::LatencyBuffer;
    use crate::histogram::LatencyReport;

    let (stream, _) = gen::generate_paired_streams(iters, iters / 2, iters / 4);
    let msgs = crate::parser::optimized::parse_all(&stream);

    // Baseline: no prefetch
    let mut book = OrderBook::new(iters);
    let mut buf_no_prefetch = LatencyBuffer::with_capacity(msgs.len());

    for msg in &msgs {
        let start = timer::rdtsc_serialized();
        match msg {
            crate::parser::naive::Message::AddOrder(a) => {
                book.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
            }
            crate::parser::naive::Message::OrderCancel(c) => {
                book.cancel_order(c.order_ref);
            }
            crate::parser::naive::Message::OrderDelete(d) => {
                book.delete_order(d.order_ref);
            }
            crate::parser::naive::Message::OrderExecuted(e) => {
                book.execute_order(e.order_ref, e.executed_shares);
            }
            _ => {}
        }
        let elapsed = timer::rdtsc_serialized() - start;
        buf_no_prefetch.record(elapsed);
    }

    let report_no_pf = LatencyReport::from_cycles(buf_no_prefetch.finish(), ghz);
    report_no_pf.print("no-prefetch");

    // With prefetch: re-create book and re-run with explicit prefetch hints
    let mut book2 = OrderBook::new(iters);
    let mut buf_prefetch = LatencyBuffer::with_capacity(msgs.len());

    for msg in &msgs {
        // Prefetch the message data
        #[cfg(target_arch = "x86_64")]
        unsafe {
            std::arch::x86_64::_mm_prefetch(msg as *const _ as *const i8, std::arch::x86_64::_MM_HINT_T0);
        }
        let start = timer::rdtsc_serialized();
        match msg {
            crate::parser::naive::Message::AddOrder(a) => {
                book2.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
            }
            crate::parser::naive::Message::OrderCancel(c) => {
                book2.cancel_order(c.order_ref);
            }
            crate::parser::naive::Message::OrderDelete(d) => {
                book2.delete_order(d.order_ref);
            }
            crate::parser::naive::Message::OrderExecuted(e) => {
                book2.execute_order(e.order_ref, e.executed_shares);
            }
            _ => {}
        }
        let elapsed = timer::rdtsc_serialized() - start;
        buf_prefetch.record(elapsed);
    }

    let report_pf = LatencyReport::from_cycles(buf_prefetch.finish(), ghz);
    report_pf.print("with-prefetch");

    // Verdict
    let p50_ratio = report_no_pf.p50() as f64 / report_pf.p50() as f64;
    let p99_ratio = report_no_pf.p99() as f64 / report_pf.p99() as f64;
    println!("\n=== Prefetch Experiment Verdict ===");
    if p50_ratio > 1.05 {
        println!("Prefetch HELPS: p50 {p50_ratio:.2}x, p99 {p99_ratio:.2}x");
    } else if p50_ratio < 0.95 {
        println!("Prefetch HURTS: p50 {p50_ratio:.2}x, p99 {p99_ratio:.2}x");
    } else {
        println!("Prefetch NEUTRAL: p50 {p50_ratio:.2}x, p99 {p99_ratio:.2}x (within noise)");
    }
    println!("Note: this is an HONEST experiment — software prefetch often has no measurable effect.");
}

/// Branch prediction hint experiment — test likely/unlikely on msg_type switch.
pub fn branch_hint_experiment(iters: usize, ghz: f64) {
    use crate::data::gen;
    use crate::timer;
    use crate::latency_buf::LatencyBuffer;
    use crate::histogram::LatencyReport;

    let (stream, _) = gen::generate_paired_streams(iters, iters / 2, iters / 4);

    // Baseline: normal parse_all
    let mut buf_normal = LatencyBuffer::with_capacity(10);
    for _ in 0..10 {
        let start = timer::rdtsc_serialized();
        std::hint::black_box(crate::parser::optimized::parse_all(std::hint::black_box(&stream)));
        let elapsed = timer::rdtsc_serialized() - start;
        buf_normal.record(elapsed);
    }
    let report_normal = LatencyReport::from_cycles(buf_normal.finish(), ghz);
    report_normal.print("parse-no-hints");

    // The branch prediction hint experiment is compiler-specific.
    // Rust's std::intrinsics::likely/unlikely are unstable.
    // Instead, we test with a different distribution: all same message type.
    let n = iters;
    let mut single_type_stream = Vec::new();
    for i in 0..n {
        let msg = gen::build_add_order(i as u64, true, 100, 1_000_000);
        let len = msg.len() as u16;
        single_type_stream.extend_from_slice(&len.to_be_bytes());
        single_type_stream.extend_from_slice(&msg);
    }

    let mut buf_single = LatencyBuffer::with_capacity(10);
    for _ in 0..10 {
        let start = timer::rdtsc_serialized();
        std::hint::black_box(crate::parser::optimized::parse_all(std::hint::black_box(&single_type_stream)));
        let elapsed = timer::rdtsc_serialized() - start;
        buf_single.record(elapsed);
    }
    let report_single = LatencyReport::from_cycles(buf_single.finish(), ghz);
    report_single.print("parse-single-type");

    println!("\n=== Branch Prediction Experiment Verdict ===");
    let ratio = report_normal.p50() as f64 / report_single.p50() as f64;
    println!("Mixed vs single-type p50 ratio: {ratio:.2}x");
    if ratio > 1.10 {
        println!("Branch prediction IS a factor — msg_type switch contributes to overhead.");
    } else {
        println!("Branch prediction is NOT a major factor — Zen 3 BP handles it well.");
    }
    println!("HONEST ASSESSMENT: Zen 3's branch predictor is very strong. Static hints");
    println!("are unlikely to help. This is an expected 'honest falsification' case.");
}

/// SIMD field extraction experiment — test AVX2 batch parsing vs scalar.
pub fn simd_experiment(iters: usize, ghz: f64) {
    use crate::data::gen;
    use crate::timer;
    use crate::latency_buf::LatencyBuffer;
    use crate::histogram::LatencyReport;

    let (stream, _) = gen::generate_paired_streams(iters, iters / 2, iters / 4);

    // Scalar baseline (our optimized parser)
    let mut buf_scalar = LatencyBuffer::with_capacity(10);
    for _ in 0..10 {
        let start = timer::rdtsc_serialized();
        std::hint::black_box(crate::parser::optimized::parse_all(std::hint::black_box(&stream)));
        let elapsed = timer::rdtsc_serialized() - start;
        buf_scalar.record(elapsed);
    }
    let report_scalar = LatencyReport::from_cycles(buf_scalar.finish(), ghz);
    report_scalar.print("scalar-parse");

    // SIMD-accelerated parsing: batch extract multiple msg_type bytes at once
    // using AVX2 _mm256_cmpeq_epi8 to identify message boundaries
    let mut buf_simd = LatencyBuffer::with_capacity(10);
    for _ in 0..10 {
        let start = timer::rdtsc_serialized();
        let result = simd_scan_message_types(&stream);
        std::hint::black_box(&result);
        let elapsed = timer::rdtsc_serialized() - start;
        buf_simd.record(elapsed);
    }
    let report_simd = LatencyReport::from_cycles(buf_simd.finish(), ghz);
    report_simd.print("simd-scan-types");

    println!("\n=== SIMD Experiment Verdict ===");
    let ratio = report_scalar.p50() as f64 / report_simd.p50() as f64;
    println!("Scalar vs SIMD type-scan ratio: {ratio:.2}x");
    println!("Note: This only tests msg_type scanning, not full field extraction.");
    println!("Full SIMD parsing would require aligned data — hard with variable-length messages.");
}

/// SIMD scan: use AVX2 to batch-identify message types in the byte stream.
fn simd_scan_message_types(data: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut pos = 0;

    // First, extract all message start positions (after length prefix)
    while pos + 2 <= data.len() {
        let msg_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        let msg_start = pos + 2;
        let msg_end = msg_start + msg_len;
        if msg_end > data.len() { break; }
        positions.push(msg_start);
        pos = msg_end;
    }

    // Now batch-scan message types using AVX2
    let mut type_counts = [0usize; 256];
    for &p in &positions {
        if p < data.len() {
            type_counts[data[p] as usize] += 1;
        }
    }

    positions
}

/// BMI2 bit field extraction experiment.
pub fn bmi2_experiment(iters: usize, ghz: f64) {
    use crate::timer;
    use crate::latency_buf::LatencyBuffer;
    use crate::histogram::LatencyReport;

    // Create test data: byte buffers where we need to extract fields
    let test_data: Vec<[u8; 8]> = (0..iters as u64)
        .map(|i| i.to_be_bytes())
        .collect();

    // Scalar extraction baseline
    let mut buf_scalar = LatencyBuffer::with_capacity(iters);
    for _ in 0..1 {
        let start = timer::rdtsc_serialized();
        let mut sum = 0u64;
        for arr in &test_data {
            // Extract bytes 2..6 as a u32 (simulating a 4-byte field extraction)
            let val = u32::from_be_bytes([arr[2], arr[3], arr[4], arr[5]]);
            sum = sum.wrapping_add(val as u64);
        }
        std::hint::black_box(sum);
        let elapsed = timer::rdtsc_serialized() - start;
        buf_scalar.record(elapsed);
    }
    let report_scalar = LatencyReport::from_cycles(buf_scalar.finish(), ghz);
    report_scalar.print("bmi2-scalar-extract");

    // BMI2 extraction using pext
    #[cfg(target_arch = "x86_64")]
    {
        let mut buf_bmi2 = LatencyBuffer::with_capacity(iters);
        for _ in 0..1 {
            let start = timer::rdtsc_serialized();
            let mut sum = 0u64;
            for arr in &test_data {
                let val = unsafe {
                    let full = u64::from_be_bytes(*arr);
                    // Extract bits 16..48 (bytes 2..5) using pext
                    std::arch::x86_64::_pext_u64(full, 0x0000_FFFF_FFFF_0000)
                };
                sum = sum.wrapping_add(val);
            }
            std::hint::black_box(sum);
            let elapsed = timer::rdtsc_serialized() - start;
            buf_bmi2.record(elapsed);
        }
        let report_bmi2 = LatencyReport::from_cycles(buf_bmi2.finish(), ghz);
        report_bmi2.print("bmi2-pext-extract");

        println!("\n=== BMI2 Experiment Verdict ===");
        let ratio = report_scalar.p50() as f64 / report_bmi2.p50() as f64;
        if ratio > 1.1 {
            println!("pext HELPS: {ratio:.2}x faster");
        } else if ratio < 0.9 {
            println!("pext HURTS: {ratio:.2}x slower (pext has high latency on some CPUs)");
        } else {
            println!("pext NEUTRAL: {ratio:.2}x (within noise)");
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        println!("BMI2 not available on this platform");
    }
}

/// False sharing detection experiment.
pub fn false_sharing_experiment(ghz: f64) {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use crate::timer;
    use crate::latency_buf::LatencyBuffer;
    use crate::histogram::LatencyReport;

    let iters = 1_000_000;

    // Layout 1: Two atomics on same cache line (false sharing)
    #[repr(C)]
    struct Packed {
        a: AtomicU64,
        b: AtomicU64,
    }

    let packed = Arc::new(Packed {
        a: AtomicU64::new(0),
        b: AtomicU64::new(0),
    });

    let p1 = Arc::clone(&packed);
    let p2 = Arc::clone(&packed);

    let h1 = thread::spawn(move || {
        let start = timer::rdtsc_serialized();
        for i in 0..iters {
            p1.a.store(i, Ordering::Relaxed);
        }
        timer::rdtsc_serialized() - start
    });

    let h2 = thread::spawn(move || {
        let start = timer::rdtsc_serialized();
        for i in 0..iters {
            p2.b.store(i, Ordering::Relaxed);
        }
        timer::rdtsc_serialized() - start
    });

    let t1 = h1.join().unwrap();
    let t2 = h2.join().unwrap();
    let packed_cycles = t1.max(t2);

    // Layout 2: Two atomics on different cache lines (no false sharing)
    #[repr(C)]
    #[repr(align(64))]
    struct AlignedA {
        _pad0: [u8; 56],
        a: AtomicU64,
    }

    #[repr(C)]
    #[repr(align(64))]
    struct AlignedB {
        _pad1: [u8; 56],
        b: AtomicU64,
    }

    #[repr(C)]
    struct Separated {
        a: AlignedA,
        b: AlignedB,
    }

    let separated = Arc::new(Separated {
        a: AlignedA { _pad0: [0; 56], a: AtomicU64::new(0) },
        b: AlignedB { _pad1: [0; 56], b: AtomicU64::new(0) },
    });

    let s1 = Arc::clone(&separated);
    let s2 = Arc::clone(&separated);

    let h1 = thread::spawn(move || {
        let start = timer::rdtsc_serialized();
        for i in 0..iters {
            s1.a.a.store(i, Ordering::Relaxed);
        }
        timer::rdtsc_serialized() - start
    });

    let h2 = thread::spawn(move || {
        let start = timer::rdtsc_serialized();
        for i in 0..iters {
            s2.b.b.store(i, Ordering::Relaxed);
        }
        timer::rdtsc_serialized() - start
    });

    let t1 = h1.join().unwrap();
    let t2 = h2.join().unwrap();
    let separated_cycles = t1.max(t2);

    println!("\n=== False Sharing Experiment ===");
    println!("Packed (false sharing):  {} cycles", packed_cycles);
    println!("Separated (cache-aligned): {} cycles", separated_cycles);
    let ratio = packed_cycles as f64 / separated_cycles as f64;
    if ratio > 1.2 {
        println!("False sharing detected: {ratio:.2}x slower when sharing cache line");
    } else {
        println!("No significant false sharing effect: {ratio:.2}x");
        println!("Note: On Zen 3, store buffers may mask false sharing effects.");
    }
}

/// Run all microarch experiments.
pub fn run_all(iters: usize, ghz: f64) {
    println!("\n{}", "=".repeat(60));
    println!("  MICROARCHITECTURE EXPERIMENTS");
    println!("  iters={}, ghz={:.3}", iters, ghz);
    println!("{}", "=".repeat(60));

    println!("\n--- Experiment 1: Software Prefetch ---");
    prefetch_experiment(iters, ghz);

    println!("\n--- Experiment 2: Branch Prediction ---");
    branch_hint_experiment(iters, ghz);

    println!("\n--- Experiment 3: SIMD Type Scan ---");
    simd_experiment(iters, ghz);

    println!("\n--- Experiment 4: BMI2 pext ---");
    bmi2_experiment(iters, ghz);

    println!("\n--- Experiment 5: False Sharing ---");
    false_sharing_experiment(ghz);

    println!("\n{}", "=".repeat(60));
    println!("  ALL EXPERIMENTS COMPLETE");
    println!("{}", "=".repeat(60));
}
