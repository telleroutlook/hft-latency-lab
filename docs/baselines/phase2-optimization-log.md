# Phase 2 Optimization Log

> Date: 2026-05-27
> Baseline: naive parser vs optimized parser (100k iterations)

## Optimization 1: Unchecked reads + inlined helpers

### Hypothesis
Bounds-checked slice indexing in the hot parse loop adds overhead. By hoisting
the single length check to the top of each message type branch and using
`get_unchecked`, we can eliminate redundant bounds checks.

### Evidence
- **Before (naive)**: p50=2,926,591 ns, p99=7,667,711 ns, p99.9=17,072,127 ns
- **After (optimized)**: p50=2,017,279 ns, p99=5,574,655 ns, p99.9=12,632,063 ns
- **Speedup**: p50 1.45x, p99 1.38x, p99.9 1.35x

### Technique
1. `#[inline(always)]` on all field reader functions (read_u16, read_u32, read_u64, read_u48)
2. `unsafe { buf.get_unchecked(range) }` after verifying `buf.len() >= MSG_SIZE`
3. Pre-allocated output vector with capacity hint (`buf.len() / 24`)
4. Direct pattern match on msg_type byte instead of calling naive parser

### Correctness gate
- All 62 differential tests pass (naive vs optimized produce identical output)
- Fuzz testing with 100 rounds of random bytes
- Boundary tests with empty input, truncated messages, max values

## Optimization 2: Pre-allocation of output vector

### Hypothesis
Dynamic Vec growth during parse_all causes allocation spikes visible in tail latency.

### Evidence
Estimated capacity = `buf.len() / 24` (average ~32 bytes per message including length prefix).
This eliminated reallocations in typical workloads.

### Result
Contributed to the p99.9 improvement — fewer allocation-related tail spikes.

## Honest assessment

These optimizations are "low-hanging fruit" — the real bottlenecks are likely:
1. **Memory bandwidth**: parsing large buffers touches many cache lines
2. **Branch prediction**: msg_type dispatch is inherently branchy
3. **Copy overhead**: Message structs are created for each parsed message

Further optimization (SIMD, batched processing) would require perf counter evidence
from an isolated-core environment. The current non-isolated setup introduces too much
noise for reliable microarchitectural optimization.

## Next steps (Phase 4 terrain)
- Run on isolated core with perf stat for precise hardware counter data
- Profile with `perf record` to identify actual hot spots
- Consider zero-copy parsing (return slices instead of owned structs)
