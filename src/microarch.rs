//! Microarchitecture experiments for Phase 4.
//!
//! Each experiment is designed to produce measurable perf counter data.
//! Run with: `./hft-latency-lab microarch --experiment <name>`

/// Software prefetch experiment — measure whether _mm_prefetch helps order book traversal.
/// Uses interleaved A/B runs to avoid cache state leaking between conditions.
pub fn prefetch_experiment(iters: usize, ghz: f64) {
    use crate::data::gen;
    use crate::histogram::LatencyReport;
    use crate::latency_buf::LatencyBuffer;
    use crate::orderbook::book::OrderBook;
    use crate::parser::naive::Message;
    use crate::timer;

    let (stream, _) = gen::generate_paired_streams(iters, iters / 2, iters / 4);
    let msgs = crate::parser::optimized::parse_all(&stream);

    let n_rounds = 5;
    let mut buf_no_prefetch = LatencyBuffer::with_capacity(msgs.len() * n_rounds);
    let mut buf_prefetch = LatencyBuffer::with_capacity(msgs.len() * n_rounds);

    for round in 0..n_rounds {
        // Alternate: even rounds = no-prefetch first, odd = prefetch first
        if round % 2 == 0 {
            // No-prefetch run (fresh book)
            let mut book = OrderBook::new(iters);
            for msg in &msgs {
                let start = timer::rdtsc_serialized();
                match msg {
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
                let elapsed = timer::rdtsc_serialized() - start;
                buf_no_prefetch.record(elapsed);
            }

            // Prefetch run (fresh book)
            let mut book2 = OrderBook::new(iters);
            for msg in &msgs {
                // Issue prefetch BEFORE the timing window — prefetch is asynchronous
                #[cfg(target_arch = "x86_64")]
                unsafe {
                    std::arch::x86_64::_mm_prefetch(
                        msg as *const _ as *const i8,
                        std::arch::x86_64::_MM_HINT_T0,
                    );
                }
                let start = timer::rdtsc_serialized();
                match msg {
                    Message::AddOrder(a) => {
                        book2.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
                    }
                    Message::OrderCancel(c) => {
                        book2.cancel_order(c.order_ref);
                    }
                    Message::OrderDelete(d) => {
                        book2.delete_order(d.order_ref);
                    }
                    Message::OrderExecuted(e) => {
                        book2.execute_order(e.order_ref, e.executed_shares);
                    }
                    _ => {}
                }
                let elapsed = timer::rdtsc_serialized() - start;
                buf_prefetch.record(elapsed);
            }
        } else {
            // Reversed order: prefetch first, then no-prefetch
            let mut book2 = OrderBook::new(iters);
            for msg in &msgs {
                #[cfg(target_arch = "x86_64")]
                unsafe {
                    std::arch::x86_64::_mm_prefetch(
                        msg as *const _ as *const i8,
                        std::arch::x86_64::_MM_HINT_T0,
                    );
                }
                let start = timer::rdtsc_serialized();
                match msg {
                    Message::AddOrder(a) => {
                        book2.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
                    }
                    Message::OrderCancel(c) => {
                        book2.cancel_order(c.order_ref);
                    }
                    Message::OrderDelete(d) => {
                        book2.delete_order(d.order_ref);
                    }
                    Message::OrderExecuted(e) => {
                        book2.execute_order(e.order_ref, e.executed_shares);
                    }
                    _ => {}
                }
                let elapsed = timer::rdtsc_serialized() - start;
                buf_prefetch.record(elapsed);
            }

            let mut book = OrderBook::new(iters);
            for msg in &msgs {
                let start = timer::rdtsc_serialized();
                match msg {
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
                let elapsed = timer::rdtsc_serialized() - start;
                buf_no_prefetch.record(elapsed);
            }
        }
    }

    let report_no_pf = LatencyReport::from_cycles(buf_no_prefetch.finish(), ghz);
    report_no_pf.print("no-prefetch");

    let report_pf = LatencyReport::from_cycles(buf_prefetch.finish(), ghz);
    report_pf.print("with-prefetch");

    // Verdict
    let p50_ratio = report_no_pf.p50() as f64 / report_pf.p50() as f64;
    let p99_ratio = report_no_pf.p99() as f64 / report_pf.p99() as f64;
    println!("\n=== Prefetch Experiment Verdict ===");
    println!(
        "Methodology: interleaved A/B/A/B runs ({} rounds), prefetch BEFORE timing window",
        n_rounds
    );
    if p50_ratio > 1.05 {
        println!("Prefetch HELPS: p50 {p50_ratio:.2}x, p99 {p99_ratio:.2}x");
    } else if p50_ratio < 0.95 {
        println!("Prefetch HURTS: p50 {p50_ratio:.2}x, p99 {p99_ratio:.2}x");
    } else {
        println!("Prefetch NEUTRAL: p50 {p50_ratio:.2}x, p99 {p99_ratio:.2}x (within noise)");
    }
    println!(
        "HONEST ASSESSMENT: software prefetch on sequential msg iteration rarely helps — \
         the hardware prefetcher already handles linear access patterns."
    );
}

/// Branch predictor warmup experiment — homogeneous vs heterogeneous message streams.
/// Tests whether the BTB learns a single-type stream faster than mixed types.
/// This is NOT about static likely/unlikely hints (Rust lacks stable intrinsics for those).
pub fn branch_predictor_experiment(iters: usize, ghz: f64) {
    use crate::data::gen;
    use crate::histogram::LatencyReport;
    use crate::latency_buf::LatencyBuffer;
    use crate::timer;

    let (stream, _) = gen::generate_paired_streams(iters, iters / 2, iters / 4);

    // Baseline: normal parse_all
    let mut buf_normal = LatencyBuffer::with_capacity(10);
    for _ in 0..10 {
        let start = timer::rdtsc_serialized();
        std::hint::black_box(crate::parser::optimized::parse_all(std::hint::black_box(
            &stream,
        )));
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
        std::hint::black_box(crate::parser::optimized::parse_all(std::hint::black_box(
            &single_type_stream,
        )));
        let elapsed = timer::rdtsc_serialized() - start;
        buf_single.record(elapsed);
    }
    let report_single = LatencyReport::from_cycles(buf_single.finish(), ghz);
    report_single.print("parse-single-type");

    println!("\n=== Branch Predictor Warmup Experiment Verdict ===");
    let ratio = report_normal.p50() as f64 / report_single.p50() as f64;
    println!("Mixed vs single-type p50 ratio: {ratio:.2}x");
    if ratio > 1.10 {
        println!("BTB warmup IS a factor — msg_type switch contributes to overhead.");
    } else {
        println!("BTB warmup is NOT a major factor — Zen 3 BP handles it well.");
    }
    println!("HONEST ASSESSMENT: This measures BTB warmup with homogeneous vs mixed streams,");
    println!("not static likely/unlikely hints (Rust lacks stable intrinsics for those).");
    println!(
        "Zen 3's branch predictor is very strong — this is an expected 'honest falsification'."
    );
}

/// SIMD field extraction experiment — test AVX2 batch msg_type scanning vs scalar.
pub fn simd_experiment(iters: usize, ghz: f64) {
    use crate::data::gen;
    use crate::histogram::LatencyReport;
    use crate::latency_buf::LatencyBuffer;
    use crate::timer;

    let (stream, _) = gen::generate_paired_streams(iters, iters / 2, iters / 4);

    // First pass: extract msg_type positions for both scalar and SIMD to work on
    let positions: Vec<usize> = {
        let mut pos = 0;
        let mut out = Vec::new();
        while pos + 2 <= stream.len() {
            let msg_len = u16::from_be_bytes([stream[pos], stream[pos + 1]]) as usize;
            let msg_start = pos + 2;
            let msg_end = msg_start + msg_len;
            if msg_end > stream.len() {
                break;
            }
            out.push(msg_start);
            pos = msg_end;
        }
        out
    };

    // Build contiguous type byte buffer once — outside any timing window.
    // Both scalar and SIMD paths read from this, so the comparison is fair.
    let type_bytes: Vec<u8> = positions
        .iter()
        .filter(|&&p| p < stream.len())
        .map(|&p| stream[p])
        .collect();

    // Scalar baseline: count msg_types one-by-one
    let mut buf_scalar = LatencyBuffer::with_capacity(10);
    for _ in 0..10 {
        let mut type_counts = [0u32; 256];
        let start = timer::rdtsc_serialized();
        for &b in &type_bytes {
            type_counts[b as usize] += 1;
        }
        std::hint::black_box(&type_counts);
        let elapsed = timer::rdtsc_serialized() - start;
        buf_scalar.record(elapsed);
    }
    let report_scalar = LatencyReport::from_cycles(buf_scalar.finish(), ghz);
    report_scalar.print("scalar-type-count");

    // SIMD: use AVX2 _mm256_cmpeq_epi8 to count msg_types 32 at a time
    #[cfg(target_arch = "x86_64")]
    {
        let mut buf_simd = LatencyBuffer::with_capacity(10);
        for _ in 0..10 {
            let start = timer::rdtsc_serialized();
            let counts = simd_count_types_avx2(&type_bytes);
            std::hint::black_box(&counts);
            let elapsed = timer::rdtsc_serialized() - start;
            buf_simd.record(elapsed);
        }
        let report_simd = LatencyReport::from_cycles(buf_simd.finish(), ghz);
        report_simd.print("simd-avx2-type-count");

        println!("\n=== SIMD Experiment Verdict ===");
        let ratio = report_scalar.p50() as f64 / report_simd.p50() as f64;
        if ratio > 1.1 {
            println!("AVX2 type-scan HELPS: {ratio:.2}x faster");
        } else if ratio < 0.9 {
            println!("AVX2 type-scan HURTS: {ratio:.2}x slower");
        } else {
            println!("AVX2 type-scan NEUTRAL: {ratio:.2}x (within noise)");
        }
        println!("Note: msg_type scan is a micro-benchmark of AVX2 batch comparison.");
        println!("Full SIMD parsing is hard with variable-length ITCH messages.");
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (iters, ghz);
        println!("AVX2 not available on this platform");
    }
}

/// AVX2 batch msg_type counting: load 32 type bytes at a time and compare
/// against known types using _mm256_cmpeq_epi8.
/// Expects a pre-built contiguous type byte buffer (built outside the timing window).
#[cfg(target_arch = "x86_64")]
fn simd_count_types_avx2(type_bytes: &[u8]) -> [u32; 256] {
    use std::arch::x86_64::*;

    let mut counts = [0u32; 256];

    // Count each known ITCH type using AVX2
    let known_types: &[u8] = b"SLAFECXDPQB";

    for &target in known_types {
        let target_vec = unsafe { _mm256_set1_epi8(target as i8) };
        let mut total = 0u32;
        let mut i = 0;
        let n = type_bytes.len();

        while i + 32 <= n {
            unsafe {
                let chunk = _mm256_loadu_si256(type_bytes.as_ptr().add(i) as *const __m256i);
                let eq = _mm256_cmpeq_epi8(chunk, target_vec);
                let mask = _mm256_movemask_epi8(eq);
                total += mask.count_ones();
            }
            i += 32;
        }

        // Scalar tail
        while i < n {
            if type_bytes[i] == target {
                total += 1;
            }
            i += 1;
        }

        counts[target as usize] = total;
    }

    counts
}

/// BMI2 bit field extraction experiment.
///
/// Known: `pext` is microcoded on AMD Zen/Zen2/Zen3 (~18 cycle latency vs ~3 on Intel Haswell+).
/// This experiment is expected to show "HURTS" on AMD — reproducing a known architectural fact,
/// not an exploratory finding. On Intel Haswell+, the conclusion would likely reverse.
pub fn bmi2_experiment(iters: usize, ghz: f64) {
    use crate::histogram::LatencyReport;
    use crate::latency_buf::LatencyBuffer;
    use crate::timer;

    // Create test data: byte buffers where we need to extract fields
    let test_data: Vec<[u8; 8]> = (0..iters as u64).map(|i| i.to_be_bytes()).collect();

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
pub fn false_sharing_experiment(_ghz: f64) {
    use crate::timer;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;

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
        a: AlignedA {
            _pad0: [0; 56],
            a: AtomicU64::new(0),
        },
        b: AlignedB {
            _pad1: [0; 56],
            b: AtomicU64::new(0),
        },
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

    println!("\n--- Experiment 2: Branch Predictor Warmup ---");
    branch_predictor_experiment(iters, ghz);

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
