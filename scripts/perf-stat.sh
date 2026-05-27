#!/usr/bin/env bash
# Wrapper: run bench binary under perf stat with full hardware counters.
# Usage: ./scripts/perf-stat.sh ./target/release/bench [args...]
set -u

BIN="${1:?Usage: $0 <bench-binary> [args...]}"
shift

perf stat -d \
    -e cycles,instructions,ipc,branch-misses,cache-misses,cache-references,\
L1-dcache-load-misses,L1-dcache-loads,LLC-load-misses,LLC-loads,\
dTLB-load-misses,iTLB-load-misses,L1-icache-load-misses,\
stalled-cycles-frontend,stalled-cycles-backend \
    taskset -c 2 "$BIN" "$@"
