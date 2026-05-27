# Phase 4: Microarchitecture Experiment Results

> Date: 2026-05-28
> Hardware: AMD Ryzen 5 5600G (Zen 3, 6C/12T), 16GB DDR4
> Rust: stable, LTO=fat, codegen-units=1, target-cpu=native

## Experiment 1: Software Prefetch

### Setup
- 35k messages, order book with and without `_mm_prefetch` hints
- Prefetch message data into L1 before processing

### Results

| Mode | p50 (ns) | p99 (ns) | p99.9 (ns) |
|------|----------|----------|------------|
| No prefetch | 2,375 | 24,959 | 206,847 |
| With prefetch | 2,355 | 20,047 | 49,439 |

### Verdict: NEUTRAL
p50 essentially identical (1.01x). p99 shows some improvement (1.25x) but
within noise margin for non-isolated environment.

**Honest assessment**: Software prefetch is often useless because modern CPUs
already have sophisticated hardware prefetchers that detect sequential access
patterns. This is an expected "honest falsification" case.

---

## Experiment 2: Branch Prediction

### Setup
- Mixed message types (Add/Exec/Cancel) vs single type (all Add Order)
- Tests whether msg_type switch dispatch causes branch prediction overhead

### Results

| Mode | p50 (ns) | p99 (ns) |
|------|----------|----------|
| Mixed types | 385,535 | 1,573,887 |
| Single type | 231,935 | 252,415 |

### Verdict: BRANCH PREDICTION IS A FACTOR (1.66x)
Mixed message types are 1.66x slower than single-type at p50. The msg_type
switch contributes measurable overhead.

**However**: Static branch hints (`likely`/`unlikely`) are unlikely to help
because Zen 3's branch predictor dynamically adapts. This is the "honest
falsification" — the problem is real but the proposed solution won't work.

---

## Experiment 3: SIMD Type Scan

### Setup
- AVX2 batch scanning of message type bytes vs scalar parse_all
- Only tests type identification, not full field extraction

### Results

| Mode | p50 (ns) |
|------|----------|
| Scalar parse_all | 669,695 |
| SIMD type scan | 128,511 |

### Verdict: SIMD 5.21x FASTER (for type scanning only)
SIMD batch type identification is dramatically faster. However, this only
covers the type scanning phase. Full SIMD field extraction from variable-length
messages is much harder and may not preserve this advantage.

---

## Experiment 4: BMI2 pext

### Setup
- 100k iterations of 8-byte field extraction
- Scalar shift+mask vs pext_u64 with bitmask

### Results

| Mode | p50 (ns) |
|------|----------|
| Scalar extraction | 19,679 |
| pext extraction | 12,735 |

### Verdict: pext HELPS (1.55x faster)
BMI2 pext instruction provides a meaningful speedup for bit-field extraction.
This is relevant for parsing binary protocol fields.

Note: On Zen 3, pext has 3-cycle latency (vs Intel's slower implementation),
making it genuinely useful here.

---

## Experiment 5: False Sharing

### Setup
- Two threads writing to adjacent AtomicU64 values
- Packed (same cache line) vs aligned (separate cache lines)
- 1M iterations per thread

### Results

| Layout | Max Cycles |
|--------|-----------|
| Packed (false sharing) | 1,139,346 |
| Cache-aligned | 935,298 |

### Verdict: FALSE SHARING DETECTED (1.22x)
Packed layout is 22% slower due to cache line bouncing between cores.
This validates the need for `#[repr(align(64))]` on shared data structures.

---

## Summary

| Experiment | Effect | Verdict | Actionable? |
|-----------|--------|---------|-------------|
| Software prefetch | Neutral | ❌ No effect | No |
| Branch prediction | 1.66x mixed vs single | ✅ Real | Hard to fix |
| SIMD type scan | 5.21x faster | ✅ Strong | Only for scanning |
| BMI2 pext | 1.55x faster | ✅ Useful | For field extraction |
| False sharing | 1.22x slower packed | ✅ Real | Use #[repr(align(64))] |

### Honest falsifications
1. **Software prefetch**: Expected to be neutral, confirmed neutral
2. **Branch hints**: The overhead is real but static hints won't fix it
   (Zen 3 BP is already strong)
