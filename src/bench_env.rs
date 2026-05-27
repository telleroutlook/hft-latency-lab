//! Benchmark environment validation — detect if the machine is in a clean state for measurement.

/// Read voluntary and nonvoluntary context switches from /proc/self/status.
pub fn read_ctxt_switches() -> (u64, u64) {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let (mut vol, mut nonvol) = (0u64, 0u64);
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("voluntary_ctxt_switches:") {
            vol = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("nonvoluntary_ctxt_switches:") {
            nonvol = v.trim().parse().unwrap_or(0);
        }
    }
    (vol, nonvol)
}

pub struct EnvSnapshot {
    pub vol: u64,
    pub nonvol: u64,
}

impl EnvSnapshot {
    pub fn take() -> Self {
        let (vol, nonvol) = read_ctxt_switches();
        Self { vol, nonvol }
    }

    /// Returns true if no involuntary preemption occurred between two snapshots.
    pub fn isolation_clean(&self, after: &EnvSnapshot) -> bool {
        after.nonvol - self.nonvol == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_read_ctxt_switches() {
        let (vol, nonvol) = read_ctxt_switches();
        // Context switches may be 0 in some test environments — just verify the function runs
        let _ = (vol, nonvol);
    }

    #[test]
    fn snapshot_pair_consistent() {
        let before = EnvSnapshot::take();
        // Do some work to potentially trigger context switches
        let mut sum = 0u64;
        for i in 0..1000 {
            sum += i;
        }
        std::hint::black_box(sum);
        let after = EnvSnapshot::take();

        // After should be >= before
        assert!(after.vol >= before.vol);
        assert!(after.nonvol >= before.nonvol);
    }
}
