//! TSC-based precision timing for latency measurement.
//! Shared infrastructure: HFT parser latency ↔ DB kernel micro-benchmarks use the same timer.

use core::arch::x86_64::{__rdtscp, _mm_lfence};

/// Read TSC with full serialization (rdtscp is inherently serializing).
/// lfence before/after prevents out-of-order execution from polluting the measurement window.
#[inline(always)]
pub fn rdtsc_serialized() -> u64 {
    unsafe {
        let mut aux = 0u32;
        _mm_lfence();
        let t = __rdtscp(&mut aux);
        _mm_lfence();
        t
    }
}

/// Calibrate TSC frequency: convert cycles to nanoseconds.
/// Call once at startup. With boost disabled, 5600G TSC frequency is constant.
pub fn calibrate_ghz() -> f64 {
    use std::time::Instant;
    let start_tsc = rdtsc_serialized();
    let start = Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let cycles = rdtsc_serialized() - start_tsc;
    let secs = start.elapsed().as_secs_f64();
    (cycles as f64) / secs / 1e9
}

#[inline(always)]
pub fn cycles_to_ns(cycles: u64, ghz: f64) -> f64 {
    cycles as f64 / ghz
}

/// RAII guard that measures the elapsed cycles between construction and drop.
/// Use in hot paths: `let _m = ScopeTimer::new(&ghz, &mut buf);`
pub struct ScopeTimer<'a> {
    start: u64,
    ghz: f64,
    buf: &'a mut crate::latency_buf::LatencyBuffer,
}

impl<'a> ScopeTimer<'a> {
    #[inline(always)]
    pub fn new(ghz: f64, buf: &'a mut crate::latency_buf::LatencyBuffer) -> Self {
        Self {
            start: rdtsc_serialized(),
            ghz,
            buf,
        }
    }
}

impl Drop for ScopeTimer<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        let elapsed = rdtsc_serialized() - self.start;
        self.buf.record(elapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_is_reasonable() {
        let ghz = calibrate_ghz();
        // 5600G base clock is 3.9 GHz, allow wide margin for test stability
        assert!(
            ghz > 1.0 && ghz < 10.0,
            "calibrated ghz = {ghz}, expected ~3.9"
        );
    }

    #[test]
    fn rdtsc_is_monotonic() {
        let a = rdtsc_serialized();
        let b = rdtsc_serialized();
        assert!(b >= a, "TSC must be monotonic: {a} -> {b}");
    }
}
