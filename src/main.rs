mod timer;
mod histogram;
mod latency_buf;
mod bench_env;
mod parser;
mod orderbook;
mod pipeline;
mod data;

use clap::Parser;

#[derive(Parser)]
#[command(name = "hft-latency-lab", about = "HFT latency engineering training ground")]
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
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Bench { iters, shuffled, core: _ } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            let (natural, shuffled_data) = data::gen::generate_paired_streams(
                iters / 2, iters / 4, iters / 8,
            );
            let stream = if shuffled { &shuffled_data } else { &natural };

            let mut buf = latency_buf::LatencyBuffer::with_capacity(iters);
            let before = bench_env::EnvSnapshot::take();

            for _ in 0..iters {
                let start = timer::rdtsc_serialized();
                std::hint::black_box(parser::optimized::parse_all(std::hint::black_box(stream)));
                let elapsed = timer::rdtsc_serialized() - start;
                buf.record(elapsed);
            }

            let after = bench_env::EnvSnapshot::take();
            let isolated = before.isolation_clean(&after);

            let report = histogram::LatencyReport::from_cycles(buf.finish(), ghz);
            let mode = if shuffled { "shuffled" } else { "natural" };
            report.print(&format!("parser-bench-{mode}"));

            if !isolated {
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

            let (stream, _) = data::gen::generate_paired_streams(messages, messages / 2, messages / 4);

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

            eprintln!("Order book: best_bid={:?} best_ask={:?}", book.best_bid(), book.best_ask());
            if !before.isolation_clean(&after) {
                eprintln!("WARNING: isolation broken during pipeline bench");
            }
        }

        Commands::Compare { iters } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            let (natural, _) = data::gen::generate_paired_streams(iters, iters / 2, iters / 4);

            // Naive parser benchmark
            let mut buf_naive = latency_buf::LatencyBuffer::with_capacity(iters);
            for _ in 0..iters {
                let start = timer::rdtsc_serialized();
                std::hint::black_box(parser::naive::parse_all(std::hint::black_box(&natural)));
                let elapsed = timer::rdtsc_serialized() - start;
                buf_naive.record(elapsed);
            }
            let naive_report = histogram::LatencyReport::from_cycles(buf_naive.finish(), ghz);
            naive_report.print("naive");

            // Optimized parser benchmark
            let mut buf_opt = latency_buf::LatencyBuffer::with_capacity(iters);
            for _ in 0..iters {
                let start = timer::rdtsc_serialized();
                std::hint::black_box(parser::optimized::parse_all(std::hint::black_box(&natural)));
                let elapsed = timer::rdtsc_serialized() - start;
                buf_opt.record(elapsed);
            }
            let opt_report = histogram::LatencyReport::from_cycles(buf_opt.finish(), ghz);
            opt_report.print("optimized");

            // Comparison
            let speedup_p50 = naive_report.p50() as f64 / opt_report.p50() as f64;
            let speedup_p99 = naive_report.p99() as f64 / opt_report.p99() as f64;
            let speedup_p999 = naive_report.p999() as f64 / opt_report.p999() as f64;
            println!("\n=== Comparison ===");
            println!("p50   speedup: {speedup_p50:.2}x");
            println!("p99   speedup: {speedup_p99:.2}x");
            println!("p99.9 speedup: {speedup_p999:.2}x");
        }

        Commands::PipelineDetailed { messages } => {
            let ghz = timer::calibrate_ghz();
            eprintln!("TSC calibrated: {ghz:.3} GHz");

            let (stream, _) = data::gen::generate_paired_streams(messages, messages / 2, messages / 4);

            // Per-stage latency buffers
            let _parse_buf = latency_buf::LatencyBuffer::with_capacity(messages);
            let mut book_buf = latency_buf::LatencyBuffer::with_capacity(messages);

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

            // Stage 2: Feed to order book one-by-one with per-message timing
            for msg in &msgs {
                let msg_start = timer::rdtsc_serialized();
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
                let msg_elapsed = timer::rdtsc_serialized() - msg_start;
                book_buf.record(msg_elapsed);
            }

            let after = bench_env::EnvSnapshot::take();

            // Reports
            println!("\n=== Pipeline Detailed Latency Report ===");
            println!("Total messages parsed: {}", msgs.len());
            println!("Parse batch total: {:.2} ms", timer::cycles_to_ns(parse_elapsed, ghz) / 1e6);
            println!("Parse per-msg avg: {:.2} ns", timer::cycles_to_ns(parse_elapsed, ghz) / msgs.len() as f64);

            let book_report = histogram::LatencyReport::from_cycles(book_buf.finish(), ghz);
            book_report.print("orderbook-per-msg");

            println!("\nOrder book state:");
            println!("  best_bid={:?} best_ask={:?}", book.best_bid(), book.best_ask());
            println!("  spread={:?} active_orders={}", book.spread(), book.order_count());

            if !before.isolation_clean(&after) {
                eprintln!("WARNING: isolation broken during pipeline bench");
            }
        }
    }
}
