//! Latency distribution reporting — never use averages, always report quantiles.
//! Wraps HdrHistogram for consistent p50/p99/p99.9/p99.99/max output.

use hdrhistogram::Histogram;

const SIGNIFICANT_DIGITS: u8 = 3;

pub struct LatencyReport {
    h: Histogram<u64>,
    unit: &'static str,
}

impl LatencyReport {
    pub fn from_ns(samples: &[u64]) -> Self {
        let mut h = Histogram::new(SIGNIFICANT_DIGITS).expect("histogram init");
        for &s in samples {
            h.record(s).ok();
        }
        Self { h, unit: "ns" }
    }

    pub fn from_cycles(samples: &[u64], ghz: f64) -> Self {
        let mut h = Histogram::new(SIGNIFICANT_DIGITS).expect("histogram init");
        for &s in samples {
            let ns = (s as f64 / ghz) as u64;
            h.record(ns).ok();
        }
        Self { h, unit: "ns" }
    }

    pub fn print(&self, label: &str) {
        let q = |p: f64| self.h.value_at_quantile(p);
        println!(
            "[{label}] p50={p50} p99={p99} p99.9={p99_9} p99.99={p99_99} max={max} ({unit}) n={n}",
            p50 = q(0.50),
            p99 = q(0.99),
            p99_9 = q(0.999),
            p99_99 = q(0.9999),
            max = self.h.max(),
            unit = self.unit,
            n = self.h.len(),
        );
    }

    pub fn p50(&self) -> u64 { self.h.value_at_quantile(0.50) }
    pub fn p99(&self) -> u64 { self.h.value_at_quantile(0.99) }
    pub fn p999(&self) -> u64 { self.h.value_at_quantile(0.999) }
    pub fn p9999(&self) -> u64 { self.h.value_at_quantile(0.9999) }
    pub fn max(&self) -> u64 { self.h.max() }
    pub fn count(&self) -> u64 { self.h.len() }
}
