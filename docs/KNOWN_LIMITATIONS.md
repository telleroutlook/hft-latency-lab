# Known Limitations

This document captures what the benchmarks in `hft-latency-lab` do and do not measure.
It is part of the project's intellectual honesty discipline — knowing the boundaries
of your measurements is as important as the measurements themselves.

## Timer overhead dominates small signals

`rdtsc_serialized()` (lfence + rdtscp + lfence) costs ~20-40 cycles on Zen 3.
Individual order-book operations (HashMap insert/lookup) take ~30-80 cycles.
This means the timer overhead is a significant fraction of — and can exceed —
the signal being measured. Conclusions about sub-100-cycle differences are unreliable.

**Mitigation used:** PipelineDetailed batches 64 messages per timing window to amortize
timer overhead. The resulting histogram contains *batch means*, not individual op latencies
(see next section).

## PipelineDetailed p99 is a batch-mean p99, not individual tail latency

The `orderbook-per-msg-batched-mean` label records `batch_elapsed / 64` per batch.
The p99 of these values is the p99 of 64-message batch means. Individual-operation
tail variance is smoothed out by batching. If you need true per-op tail latency,
you must use unbatched timing and accept the timer-overhead contamination above.

## Single-socket setup cannot measure NUMA effects

All benchmarks run on a single AMD Ryzen 5 5600G (Zen 3, single CCX). Cross-NUMA
memory access latency, remote cache snooping, and inter-socket coherency traffic
cannot be measured on this hardware. Findings about cache behavior apply only to
the local CCX.

## Zen 3 `pext` is microcoded — BMI2 experiment reproduces a known fact

The BMI2 `pext` instruction is microcoded on AMD Zen/Zen2/Zen3 (~18 cycle latency
vs ~3 on Intel Haswell+). The BMI2 experiment showing "HURTS" is reproducing a known
architectural fact, not an exploratory finding. On Intel, the conclusion would likely reverse.

## Software prefetch experiment prefetches sequentially-accessed data

The prefetch experiment issues `_mm_prefetch` on the next `Message` enum in a linear
`Vec<Message>` traversal. The hardware prefetcher already handles sequential access
patterns, so software prefetch is expected to be neutral. This experiment correctly
demonstrates that software prefetch on sequential access is useless, but it does not
demonstrate the real value of software prefetch, which is prefetching data the HW
prefetcher cannot predict (e.g., pointer-chasing into an arena via `order_ref`).

## SIMD experiment uses pre-built contiguous buffer

Both scalar and SIMD paths operate on a pre-built `type_bytes: Vec<u8>` rather than
gathering bytes from the raw stream during timing. This makes the comparison fair
(both paths read the same contiguous buffer), but means neither path includes stream
parsing overhead in its timing. The result is an isolated comparison of scan methods,
not a full-parser throughput number.

## IRQ count is all-CPU aggregate, not per-isolated-core

`bench_env::EnvSnapshot` sums interrupt counts across all CPUs from `/proc/interrupts`.
This is sufficient for detecting machine-wide IRQ storms, but cannot answer
"was my isolated core disturbed by interrupts?". Per-CPU column parsing would be
needed for that granularity.

## TSC calibration does not check cpuid flags

The timer calibrates TSC frequency by comparing two 1-second wall-clock passes.
This is a valid consistency check on invariant-TSC CPUs, but does not programmatically
verify `constant_tsc` / `nonstop_tsc` via cpuid. On a non-invariant-TSC CPU (e.g.,
older AMD or frequency-scaling-locked Intel), the calibration would silently produce
incorrect results.

## Branch predictor experiment measures batch throughput, not per-branch prediction

The branch_predictor_experiment times `parse_all` over the full stream (batch timing),
then compares mixed vs single-type message streams. Both sides carry the same measurement
noise (timer overhead, batch effects), so the *ratio* is meaningful. But individual
per-branch prediction latency is not captured — only the aggregate throughput difference
caused by BTB warmup behavior.
