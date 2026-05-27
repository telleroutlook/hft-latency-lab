# hft-latency-lab

HFT latency engineering training ground — systematic practice in nanosecond-level measurement, bottleneck attribution, and honest falsification, built around a NASDAQ TotalView-ITCH message parser and order book.

**Not a trading system.** This is a portfolio piece demonstrating that the author can measure precisely, optimize with evidence, and publish limitations honestly.

## What it does

| Subcommand | What it measures |
|-------------|-----------------|
| `bench` | Per-message ITCH parser latency (p50/p99/p99.9/p99.99/max) |
| `compare` | Naive vs optimized parser side-by-side |
| `diff-test` | Differential fuzzing between parser implementations |
| `env-check` | Context switch and IRQ noise audit |
| `pipeline` | End-to-end: parse + order book update |
| `pipeline-detailed` | Per-stage latency breakdown with batched book timing |
| `microarch` | 5 controlled microarchitecture experiments (prefetch, branch predictor, SIMD, BMI2, false sharing) |
| `net-bench` | io_uring simulation and packet receiver end-to-end |

Every benchmark reports **quantile distributions**, never averages.

## Build and run

```bash
cargo build --release
./target/release/hft-latency-lab bench --iters 1000000
./target/release/hft-latency-lab pipeline-detailed --messages 500000
./target/release/hft-latency-lab microarch --experiment all
./target/release/hft-latency-lab net-bench --messages 50000
```

### Environment setup (optional, for clean measurements)

```bash
# Isolate cores 2-5 for benchmarking
sudo isolcpus=2,3,4,5

# Check environment purity
./scripts/bench-env-check.sh

# Profile with hardware counters
./scripts/perf-stat.sh ./target/release/hft-latency-lab bench
```

## Architecture

```
src/
├── timer.rs             TSC-based serialized timing (lfence + rdtscp + lfence)
├── histogram.rs         HdrHistogram wrapper — always quantiles, never averages
├── latency_buf.rs       Flat-array sample buffer for hot-path zero-alloc recording
├── bench_env.rs         /proc/self/status + /proc/interrupts noise detection
├── parser/
│   ├── naive.rs         Reference implementation (never optimized, used as diff oracle)
│   ├── optimized.rs     Optimized parser with per-message timed variant
│   └── diff.rs          Differential test harness
├── orderbook/
│   ├── arena.rs         Arena + index allocator for cache-friendly order storage
│   └── book.rs          HashMap-indexed order book with BBO tracking
├── pipeline/
│   └── spsc.rs          Lock-free SPSC ring queue
├── data/
│   └── gen.rs           Paired natural-order + shuffled test streams
├── microarch.rs         5 controlled microarchitecture experiments
└── net/
    ├── io_uring_bench.rs    io_uring batch submission simulation
    ├── packet_timer.rs      Per-packet timing with batch vs single syscall
    └── raw_socket.rs        Simulated packet receiver with order book integration
```

## Methodology

**Measurement discipline** — every benchmark follows the same pattern:

1. `EnvSnapshot::take()` before and after — detect involuntary preemption or IRQ storms
2. `rdtsc_serialized()` brackets each measured region (full serialization via lfence + rdtscp)
3. TSC calibrated by two 1-second passes, consistency checked within 0.5%
4. All results reported as quantile distributions (p50/p99/p99.9/p99.99/max)

**Honest falsification** — every experiment includes a verdict and an honest assessment:
- The prefetch experiment correctly shows NEUTRAL on sequential access (HW prefetcher wins)
- The BMI2 experiment reproduces the known Zen 3 `pext` microcoded penalty
- The false sharing experiment acknowledges Zen 3 store buffer effects

See [docs/KNOWN_LIMITATIONS.md](docs/KNOWN_LIMITATIONS.md) for a complete accounting of what these benchmarks do and do not measure.

## Documentation

| Document | Content |
|----------|---------|
| [docs/plan-overview.md](docs/plan-overview.md) | Project roadmap and phase status |
| [docs/KNOWN_LIMITATIONS.md](docs/KNOWN_LIMITATIONS.md) | What the benchmarks do and do not measure |
| [docs/honest-discipline.md](docs/honest-discipline.md) | Measurement honesty rules |
| [docs/measuremente-checklist.md](docs/measuremente-checklist.md) | A-through-J measurement checklist |
| [docs/hardware-profile.md](docs/hardware-profile.md) | Target hardware capabilities |
| [docs/phase1-foundation.md](docs/phase1-foundation.md) | Phase 1: measurement infrastructure |
| [docs/phase2-itchy.md](docs/phase2-itchy.md) | Phase 2: ITCH parser optimization |
| [docs/phase3-pipeline.md](docs/phase3-pipeline.md) | Phase 3: pipeline and order book |
| [docs/phase4-microarch.md](docs/phase4-microarch.md) | Phase 4: microarchitecture experiments |
| [docs/phase5-kernel.md](docs/phase5-kernel.md) | Phase 5: network and kernel bypass |

## Test

```bash
cargo test                        # Unit tests (parser diff, histogram quantiles, timer monotonicity)
cargo test --test differential    # Cross-implementation differential fuzzing
```

## Hardware target

AMD Ryzen 5 5600G (Zen 3, 6C/12T), 16 GB RAM, 1 TB SSD. Core isolation via `isolcpus=2,3,4,5`.

The project is designed to be honest about what this hardware can and cannot measure: no NUMA, no AVX-512, no FPGA. The ceiling is userspace algorithms + microarchitecture tuning + kernel bypass — and the benchmarks are structured to stay within that envelope.

## License

MIT
