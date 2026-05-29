<div align="center">

# hft-latency-lab

**Nanosecond-Level Latency Engineering Training Ground**

[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Systematic practice in measurement, bottleneck attribution, and honest falsification — built around a NASDAQ TotalView-ITCH message parser and order book. **Not a trading system.** A portfolio piece demonstrating precise measurement and evidence-based optimization.

</div>

---

## Benchmark Suite

| Subcommand | What It Measures |
|:-----------|:-----------------|
| `bench` | Per-message ITCH parser latency (p50/p99/p99.9/p99.99/max) |
| `compare` | Naive vs optimized parser side-by-side |
| `diff-test` | Differential fuzzing between parser implementations |
| `env-check` | Context switch and IRQ noise audit |
| `pipeline` | End-to-end: parse + order book update |
| `pipeline-detailed` | Per-stage latency breakdown with batched timing |
| `microarch` | 5 controlled microarchitecture experiments |
| `net-bench` | io_uring simulation and packet receiver |

Every benchmark reports **quantile distributions** — never averages.

---

## Quick Start

```bash
git clone https://github.com/telleroutlook/hft-latency-lab.git
cd hft-latency-lab
cargo build --release
```

```bash
# Parser latency benchmark
./target/release/hft-latency-lab bench --iters 1000000

# End-to-end pipeline with per-stage breakdown
./target/release/hft-latency-lab pipeline-detailed --messages 500000

# All microarchitecture experiments
./target/release/hft-latency-lab microarch --experiment all

# Network layer benchmarks
./target/release/hft-latency-lab net-bench --messages 50000
```

---

## Microarchitecture Experiments

| Experiment | What It Tests | Honest Verdict |
|:-----------|:-------------|:---------------|
| Prefetch | Software `__builtin_prefetch` vs HW prefetcher | NEUTRAL on sequential — HW prefetcher wins |
| Branch Predictor | Pattern-dependent branch behavior | Measurable, workload-sensitive |
| SIMD | Vectorized parsing operations | Effective on batch-aligned data |
| BMI2 | `pext`/`pdep` bit extraction | PENALTY on Zen 3 (microcoded instruction) |
| False Sharing | Cache-line contention across threads | Detectable, Zen 3 store buffer mitigates |

---

## Methodology

**Measurement discipline:**

1. `EnvSnapshot::take()` before and after — detect involuntary preemption or IRQ storms
2. `rdtsc_serialized()` brackets each measured region (`lfence + rdtscp + lfence`)
3. TSC calibrated by two 1-second passes, consistency checked within 0.5%
4. All results as quantile distributions (p50/p99/p99.9/p99.99/max)

**Honest falsification:**
- The prefetch experiment correctly shows NEUTRAL on sequential access
- The BMI2 experiment reproduces the known Zen 3 `pext` penalty
- The false sharing experiment acknowledges store buffer effects
- All known limitations documented in [docs/KNOWN_LIMITATIONS.md](docs/KNOWN_LIMITATIONS.md)

---

## Architecture

```
src/
├── timer.rs             # TSC-based serialized timing
├── histogram.rs         # HdrHistogram — quantiles, never averages
├── latency_buf.rs       # Zero-alloc flat-array sample buffer
├── bench_env.rs         # /proc noise detection
├── parser/
│   ├── naive.rs         # Reference (never optimized, diff oracle)
│   ├── optimized.rs     # Optimized with per-message timing
│   └── diff.rs          # Differential test harness
├── orderbook/
│   ├── arena.rs         # Cache-friendly arena allocator
│   └── book.rs          # HashMap-indexed with BBO tracking
├── pipeline/
│   └── spsc.rs          # Lock-free SPSC ring queue
├── microarch.rs         # 5 controlled experiments
├── net/
│   ├── io_uring_bench.rs    # io_uring batch simulation
│   ├── packet_timer.rs      # Per-packet timing
│   └── raw_socket.rs        # Simulated packet receiver
└── strategy/
    ├── cointegration.rs     # Cointegration strategy
    └── pipeline.rs          # Strategy pipeline
```

---

## Environment Setup (Optional, for Clean Measurements)

```bash
# Isolate cores for benchmarking
sudo isolcpus=2,3,4,5

# Check environment purity
./scripts/bench-env-check.sh

# Profile with hardware counters
./scripts/perf-stat.sh ./target/release/hft-latency-lab bench
```

---

## Tests

```bash
# Unit tests (parser diff, histogram quantiles, timer monotonicity)
cargo test

# Cross-implementation differential fuzzing
cargo test --test differential
```

---

## Documentation

| Document | Content |
|:---------|:--------|
| [plan-overview.md](docs/plan-overview.md) | Project roadmap and phase status |
| [KNOWN_LIMITATIONS.md](docs/KNOWN_LIMITATIONS.md) | What the benchmarks do and do not measure |
| [honest-discipline.md](docs/honest-discipline.md) | Measurement honesty rules |
| [measuremente-checklist.md](docs/measuremente-checklist.md) | A-through-J measurement checklist |
| [hardware-profile.md](docs/hardware-profile.md) | Target hardware (AMD Ryzen 5 5600G, Zen 3) |
| [phase1-foundation.md](docs/phase1-foundation.md) | Phase 1: measurement infrastructure |
| [phase2-itchy.md](docs/phase2-itchy.md) | Phase 2: ITCH parser optimization |
| [phase3-pipeline.md](docs/phase3-pipeline.md) | Phase 3: pipeline and order book |
| [phase4-microarch.md](docs/phase4-microarch.md) | Phase 4: microarchitecture experiments |
| [phase5-kernel.md](docs/phase5-kernel.md) | Phase 5: network and kernel bypass |

---

## Hardware Target

AMD Ryzen 5 5600G (Zen 3, 6C/12T), 16 GB RAM, 1 TB SSD.

The project is honest about what this hardware can and cannot measure: no NUMA, no AVX-512, no FPGA. The ceiling is userspace algorithms + microarchitecture tuning + kernel bypass.

---

## License

Apache License 2.0
