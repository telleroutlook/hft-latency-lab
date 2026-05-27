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
                        book.cancel_order(d.order_ref);
                    }
                    parser::naive::Message::OrderExecuted(e) => {
                        // Partial execution reduces shares — treat as partial cancel for now
                        book.cancel_order(e.order_ref);
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
    }
}
