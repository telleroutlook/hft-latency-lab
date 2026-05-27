//! Hot-path flat array buffer for latency samples.
//! Rule: NEVER touch HdrHistogram in the hot path. Record cycles here, aggregate after.

pub struct LatencyBuffer {
    samples: Vec<u64>,
    idx: usize,
}

impl LatencyBuffer {
    pub fn with_capacity(n: usize) -> Self {
        Self {
            samples: vec![0u64; n],
            idx: 0,
        }
    }

    /// Hot path: one write, no branch, no allocation.
    #[inline(always)]
    pub fn record(&mut self, cycles: u64) {
        debug_assert!(self.idx < self.samples.len(), "latency buffer overflow");
        // Bounds check is elided in release when caller ensures capacity.
        unsafe {
            *self.samples.get_unchecked_mut(self.idx) = cycles;
        }
        self.idx += 1;
    }

    pub fn finish(&self) -> &[u64] {
        &self.samples[..self.idx]
    }

    pub fn reset(&mut self) {
        self.idx = 0;
    }

    pub fn len(&self) -> usize {
        self.idx
    }

    pub fn is_empty(&self) -> bool {
        self.idx == 0
    }
}
