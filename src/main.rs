#![allow(dead_code)]
mod bench_env;
mod data;
mod histogram;
mod latency_buf;
mod microarch;
mod net;
mod orderbook;
mod parser;
mod pipeline;
mod timer;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "hft-latency-lab",
    about = "HFT latency engineering training ground"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Run ITCH parser benchmark with latency distribution report
    Bench {
        #[arg(short, long, default_value = "1000000")]
        iters: usize,

        #[arg(long)]
        shuffled: bool,

        #[arg(long, default_value = "2")]
        core: usize,
    },

    /// Run differential test: naive vs optimized parser
    DiffTest,

    /// Check environment purity for benchmarking
    EnvCheck,

    /// Run end-to-end pipeline benchmark (parser → SPSC → order book)
    Pipeline {
        #[arg(short, long, default_value = "500000")]
        messages: usize,
    },

    /// Compare naive vs optimized parser performance
    Compare {
        #[arg(short, long, default_value = "100000")]
        iters: usize,
    },

    /// Run end-to-end pipeline with per-stage latency breakdown
    PipelineDetailed {
        #[arg(short, long, default_value = "200000")]
        messages: usize,
    },

    /// Run microarchitecture experiments (TMA, cache, branch, SIMD)
    Microarch {
        #[arg(short, long, default_value = "100000")]
        iters: usize,

        #[arg(long)]
        experiment: Option<String>,
    },

    /// Phase 5: Network layer benchmarks (io_uring sim, packet timing)
    NetBench {
        #[arg(short, long, default_value = "50000")]
        messages: usize,

        #[arg(long)]
        syscall_overhead: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Bench {
            iters,
            shuffled,
            core: _,
        } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            let (natural, shuffled_data) =
                data::gen::generate_paired_streams(iters, iters / 2, iters / 4);
            let stream = if shuffled { &shuffled_data } else { &natural };

            // Per-message latency: each sample is one parse_one call
            let mut buf = latency_buf::LatencyBuffer::with_capacity(
                stream.len() / 20, // rough message count estimate
            );
            let before = bench_env::EnvSnapshot::take();

            let _msgs = parser::optimized::parse_all_timed(stream, &mut buf);

            let after = bench_env::EnvSnapshot::take();

            let report = histogram::LatencyReport::from_cycles(buf.finish(), ghz);
            let mode = if shuffled { "shuffled" } else { "natural" };
            report.print(&format!("parser-per-msg-{mode}"));

            if !before.isolation_clean(&after) {
                eprintln!("WARNING: involuntary context switches detected — isolation broken, data unreliable");
            }
        }

        Commands::DiffTest => {
            eprintln!("Run: cargo test -- parser::diff");
        }

        Commands::EnvCheck => {
            let (vol, nonvol) = bench_env::read_ctxt_switches();
            eprintln!("Context switches: voluntary={vol} nonvoluntary={nonvol}");
            eprintln!("For full env check, run: ./scripts/bench-env-check.sh");
        }

        Commands::Pipeline { messages } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            let (stream, _) =
                data::gen::generate_paired_streams(messages, messages / 2, messages / 4);

            let mut buf = latency_buf::LatencyBuffer::with_capacity(messages);
            let mut book = orderbook::book::OrderBook::new(messages);

            let before = bench_env::EnvSnapshot::take();

            let msgs = parser::optimized::parse_all(&stream);
            for msg in &msgs {
                let start = timer::rdtsc_serialized();
                match msg {
                    parser::naive::Message::AddOrder(a) => {
                        book.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
                    }
                    parser::naive::Message::OrderCancel(c) => {
                        book.cancel_order(c.order_ref);
                    }
                    parser::naive::Message::OrderDelete(d) => {
                        book.delete_order(d.order_ref);
                    }
                    parser::naive::Message::OrderExecuted(e) => {
                        book.execute_order(e.order_ref, e.executed_shares);
                    }
                    _ => {}
                }
                let elapsed = timer::rdtsc_serialized() - start;
                buf.record(elapsed);
            }

            let after = bench_env::EnvSnapshot::take();
            let report = histogram::LatencyReport::from_cycles(buf.finish(), ghz);
            report.print("pipeline-e2e");

            eprintln!(
                "Order book: best_bid={:?} best_ask={:?}",
                book.best_bid(),
                book.best_ask()
            );
            if !before.isolation_clean(&after) {
                eprintln!("WARNING: isolation broken during pipeline bench");
            }
        }

        Commands::Compare { iters } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            let (natural, _) = data::gen::generate_paired_streams(iters, iters / 2, iters / 4);
            let msg_count_est = natural.len() / 20;

            // Naive parser: per-message timing
            let mut buf_naive = latency_buf::LatencyBuffer::with_capacity(msg_count_est);
            let _ = parser::naive::parse_all_timed(&natural, &mut buf_naive);
            let naive_report = histogram::LatencyReport::from_cycles(buf_naive.finish(), ghz);
            naive_report.print("naive-per-msg");

            // Optimized parser: per-message timing
            let mut buf_opt = latency_buf::LatencyBuffer::with_capacity(msg_count_est);
            let _ = parser::optimized::parse_all_timed(&natural, &mut buf_opt);
            let opt_report = histogram::LatencyReport::from_cycles(buf_opt.finish(), ghz);
            opt_report.print("optimized-per-msg");

            // Comparison
            let speedup_p50 = naive_report.p50() as f64 / opt_report.p50() as f64;
            let speedup_p99 = naive_report.p99() as f64 / opt_report.p99() as f64;
            let speedup_p999 = naive_report.p999() as f64 / opt_report.p999() as f64;
            println!("\n=== Per-Message Comparison ===");
            println!("p50   speedup: {speedup_p50:.2}x");
            println!("p99   speedup: {speedup_p99:.2}x");
            println!("p99.9 speedup: {speedup_p999:.2}x");
        }

        Commands::PipelineDetailed { messages } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            let (stream, _) =
                data::gen::generate_paired_streams(messages, messages / 2, messages / 4);

            let mut book_buf = latency_buf::LatencyBuffer::with_capacity(messages / 64 + 1);

            let mut book = orderbook::book::OrderBook::new(messages);
            let _bbo_count = 0u64;
            book.set_bbo_callback(Box::new(|_bid, _ask| {
                // Minimal callback — just counting
            }));

            let before = bench_env::EnvSnapshot::take();

            // Stage 1: Parse all messages (batch)
            let parse_start = timer::rdtsc_serialized();
            let msgs = parser::optimized::parse_all(&stream);
            let parse_elapsed = timer::rdtsc_serialized() - parse_start;

            // Stage 2: Feed to order book in batches of 64 to amortize timer overhead.
            // Per-message rdtsc (~20-40 cycles) dominates the actual op (~30-80 cycles),
            // so batch timing gives honest per-message estimates.
            let batch_size = 64;
            let mut msg_idx = 0;
            while msg_idx < msgs.len() {
                let batch_end = (msg_idx + batch_size).min(msgs.len());
                let batch_start = timer::rdtsc_serialized();
                for msg in &msgs[msg_idx..batch_end] {
                    match msg {
                        parser::naive::Message::AddOrder(a) => {
                            book.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
                        }
                        parser::naive::Message::OrderCancel(c) => {
                            book.cancel_order(c.order_ref);
                        }
                        parser::naive::Message::OrderDelete(d) => {
                            book.delete_order(d.order_ref);
                        }
                        parser::naive::Message::OrderExecuted(e) => {
                            book.execute_order(e.order_ref, e.executed_shares);
                        }
                        _ => {}
                    }
                }
                let batch_elapsed = timer::rdtsc_serialized() - batch_start;
                let n = (batch_end - msg_idx) as u64;
                let per_msg = batch_elapsed / n;
                book_buf.record(per_msg);
                msg_idx = batch_end;
            }

            let after = bench_env::EnvSnapshot::take();

            // Reports
            println!("\n=== Pipeline Detailed Latency Report ===");
            println!("Total messages parsed: {}", msgs.len());
            println!(
                "Parse batch total: {:.2} ms",
                timer::cycles_to_ns(parse_elapsed, ghz) / 1e6
            );
            println!(
                "Parse per-msg avg: {:.2} ns",
                timer::cycles_to_ns(parse_elapsed, ghz) / msgs.len() as f64
            );
            println!(
                "Order book timing: batched ({} msgs/batch), per-msg = batch_elapsed / batch_size",
                batch_size
            );

            let book_report = histogram::LatencyReport::from_cycles(book_buf.finish(), ghz);
            book_report.print("orderbook-per-msg-batched-mean");
            println!(
                "  Note: p99 = p99 of {}-msg batch means; individual-op tail variance is smoothed out by batching.",
                batch_size
            );

            println!("\nOrder book state:");
            println!(
                "  best_bid={:?} best_ask={:?}",
                book.best_bid(),
                book.best_ask()
            );
            println!(
                "  spread={:?} active_orders={}",
                book.spread(),
                book.order_count()
            );

            if !before.isolation_clean(&after) {
                eprintln!("WARNING: isolation broken during pipeline bench");
            }
        }

        Commands::Microarch { iters, experiment } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            match experiment.as_deref() {
                Some("prefetch") => microarch::prefetch_experiment(iters, ghz),
                Some("branch_predictor") => microarch::branch_predictor_experiment(iters, ghz),
                Some("simd") => microarch::simd_experiment(iters, ghz),
                Some("bmi2") => microarch::bmi2_experiment(iters, ghz),
                Some("false-sharing") => microarch::false_sharing_experiment(ghz),
                Some("all") | None => microarch::run_all(iters, ghz),
                _ => eprintln!(
                    "Unknown experiment. Options: prefetch, branch_predictor, simd, bmi2, false-sharing, all"
                ),
            }
        }

        Commands::NetBench {
            messages,
            syscall_overhead,
        } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            if syscall_overhead {
                println!("\n=== Syscall Overhead Baseline ===");
                net::io_uring_bench::syscall_overhead_bench(ghz);
            }

            println!("\n=== io_uring Simulation Benchmark ===");
            net::io_uring_bench::io_uring_simulated_bench(messages, ghz);

            println!("\n=== Packet Receiver End-to-End ===");
            let (stream, _) =
                data::gen::generate_paired_streams(messages, messages / 2, messages / 4);
            let mut receiver = net::raw_socket::PacketReceiver::new(messages);
            receiver.process_stream(&stream);

            let (total_report, book_report, _strategy_report) = receiver.latency_report(ghz);
            total_report.print("packet-e2e-total");
            book_report.print("packet-book-only");

            let book = receiver.book();
            println!(
                "\nOrder book: best_bid={:?} best_ask={:?} spread={:?} orders={}",
                book.best_bid(),
                book.best_ask(),
                book.spread(),
                book.order_count()
            );
        }
    }
}
