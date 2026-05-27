# Phase 1 Baseline Report

> Date: 2026-05-27
> Hardware: AMD Ryzen 5 5600G (Zen 3, 6C/12T), 16GB DDR4
> Rust: stable, LTO=fat, codegen-units=1, target-cpu=native

## Environment

| Check | Status | Value |
|-------|--------|-------|
| TSC frequency | ✅ | 3.893 GHz |
| Core isolation | ⚠️ | Not on isolated core in test env |
| Involuntary switches | ⚠️ | Detected (expected without isolcpus) |

## Parser Baseline (100k iterations, natural order)

| Metric | Value (ns) |
|--------|-----------|
| p50 | 1,474,559 |
| p99 | 4,505,599 |
| p99.9 | 9,502,719 |
| p99.99 | 10,952,703 |
| max | 13,295,615 |

Note: These numbers are from a non-isolated environment. The absolute values
will be much lower on an isolated core with performance governor. The relative
distribution shape is still informative.

## Pipeline E2E Baseline (175k messages)

| Metric | Value (ns) |
|--------|-----------|
| p50 | 58,431 |
| p99 | 172,671 |
| p99.9 | 965,631 |
| p99.99 | 2,363,391 |
| max | 4,014,079 |

## Test Coverage

| Component | Tests | Status |
|-----------|-------|--------|
| timer.rs | 2 (calibration, monotonicity) | ✅ |
| histogram.rs | 3 (quantiles, conversion, single) | ✅ |
| latency_buf.rs | 4 (basic, overflow, reset, stress) | ✅ |
| bench_env.rs | 2 (read, snapshot pair) | ✅ |
| parser/naive.rs | 5 (add, delete, unknown, short, length-prefixed) | ✅ |
| parser/diff.rs | 9 (all, one, shuffled, full, boundaries, fuzz, max, empty, unknown) | ✅ |
| orderbook/arena.rs | 1 (alloc/free cycle) | ✅ |
| orderbook/book.rs | 1 (basic operations) | ✅ |
| pipeline/spsc.rs | 2 (basic, full) | ✅ |
| tests/differential.rs | 2 (cross-crate, full stream) | ✅ |
| **Total** | **31 unique tests** | ✅ |

## Message Type Coverage

| Type | Code | Size | Status |
|------|------|------|--------|
| System Event | S | 12 | ✅ |
| Market Participant | L | 26 | ✅ |
| Add Order | A | 36 | ✅ |
| Add Order w/ MPID | F | 40 | ✅ |
| Order Executed | E | 31 | ✅ |
| Executed w/ Price | C | 36 | ✅ |
| Order Cancel | X | 23 | ✅ |
| Order Delete | D | 19 | ✅ |
| Trade | P | 44 | ✅ |
| Cross Trade | Q | 40 | ✅ |
| Broken Trade | B | 19 | ✅ |

## Known Issues

1. Benchmark runs show involuntary context switches — expected without `isolcpus`
2. Optimized parser is currently identical to naive (Phase 2 optimization target)
3. Order book cancel is O(n) linear scan — HashMap index planned for Phase 3

## Conclusion

Phase 1 infrastructure is complete:
- ✅ TSC timer with calibration and monotonicity verification
- ✅ Latency histogram with p50/p99/p99.9/p99.99/max reporting
- ✅ Hot-path latency buffer with overflow safety
- ✅ Environment detection for involuntary preemption
- ✅ Complete ITCH 5.0 parser (all 11 message types, binary format)
- ✅ Differential testing framework with fuzz, boundary, and cross-crate tests
- ✅ Test data generator covering all message types
- ✅ Arena-allocated order book with basic operations
- ✅ SPSC lock-free ring buffer
- ✅ Anti-compiler-ghost configuration (LTO, codegen-units, target-cpu=native)
