//! Signal quality gating for trade execution.
//!
//! Evaluates multi-dimensional signal quality (EKF gain, data freshness,
//! spread health, ADF cointegration strength) and produces a composite
//! score used to gate trade execution. This prevents entering positions
//! when market conditions or signal confidence are degraded.

/// Signal quality dimensions for trade execution gating.
#[derive(Debug, Clone)]
pub struct SignalQuality {
    /// EKF Kalman gain (0..1) — higher = more confident
    pub ekf_gain: f64,
    /// Data freshness: fraction of max_staleness (1.0 = fresh, 0.0 = stale)
    pub freshness: f64,
    /// Spread health: current_spread / avg_spread (1.0 = normal, >1.0 = wide)
    pub spread_health: f64,
    /// ADF strength: 1.0 - p_value (higher = stronger cointegration)
    pub adf_strength: f64,
}

impl Default for SignalQuality {
    fn default() -> Self {
        Self {
            ekf_gain: 0.0,
            freshness: 0.0,
            spread_health: 0.0,
            adf_strength: 0.0,
        }
    }
}

/// Weights for quality dimensions.
pub struct QualityWeights {
    pub gain: f64,
    pub freshness: f64,
    pub spread: f64,
    pub adf: f64,
}

impl Default for QualityWeights {
    fn default() -> Self {
        Self {
            gain: 0.3,
            freshness: 0.2,
            spread: 0.2,
            adf: 0.3,
        }
    }
}

/// Evaluates signal quality and gates trade execution.
pub struct QualityGate {
    pub weights: QualityWeights,
    pub threshold: f64,
    pub max_staleness_us: u64,
    pub avg_spread: f64,
}

impl QualityGate {
    pub fn new(threshold: f64, max_staleness_us: u64, avg_spread: f64) -> Self {
        Self {
            weights: QualityWeights::default(),
            threshold,
            max_staleness_us,
            avg_spread,
        }
    }

    /// Evaluate composite quality score (0..1).
    pub fn evaluate(&self, quality: &SignalQuality) -> f64 {
        let w = &self.weights;
        w.gain * quality.ekf_gain
            + w.freshness * quality.freshness
            + w.spread * quality.spread_health
            + w.adf * quality.adf_strength
    }

    /// Returns true if signal quality is sufficient for execution.
    pub fn should_execute(&self, quality: &SignalQuality) -> bool {
        self.evaluate(quality) >= self.threshold
    }

    /// Compute freshness from a staleness timestamp.
    /// Returns 1.0 for fresh data (staleness = 0), linearly decaying to 0.0
    /// at max_staleness_us.
    pub fn compute_freshness(&self, staleness_us: u64) -> f64 {
        if staleness_us >= self.max_staleness_us {
            0.0
        } else {
            1.0 - (staleness_us as f64 / self.max_staleness_us as f64)
        }
    }

    /// Compute spread health from current spread.
    /// Returns 1.0 when current matches average, decaying for wider spreads.
    /// Spreads wider than 2x average return 0.0. Tighter spreads (< 1x) clamp to 1.0.
    pub fn compute_spread_health(&self, current_spread: f64) -> f64 {
        if self.avg_spread <= 0.0 {
            return 1.0;
        }
        let ratio = current_spread / self.avg_spread;
        if ratio <= 1.0 {
            1.0
        } else if ratio >= 2.0 {
            0.0
        } else {
            2.0 - ratio
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_quality_passes() {
        let gate = QualityGate::new(0.6, 100_000, 0.01);
        let quality = SignalQuality {
            ekf_gain: 1.0,
            freshness: 1.0,
            spread_health: 1.0,
            adf_strength: 1.0,
        };
        assert!(gate.should_execute(&quality));
        let score = gate.evaluate(&quality);
        assert!((score - 1.0).abs() < 1e-10);
    }

    #[test]
    fn zero_quality_fails() {
        let gate = QualityGate::new(0.6, 100_000, 0.01);
        let quality = SignalQuality::default();
        assert!(!gate.should_execute(&quality));
    }

    #[test]
    fn threshold_boundary() {
        let gate = QualityGate::new(0.5, 100_000, 0.01);
        // weights: gain=0.3, freshness=0.2, spread=0.2, adf=0.3
        // 0.3*1.0 + 0.2*1.0 + 0.2*0.0 + 0.3*1.0 = 0.8
        let quality = SignalQuality {
            ekf_gain: 1.0,
            freshness: 1.0,
            spread_health: 0.0,
            adf_strength: 1.0,
        };
        assert!(gate.should_execute(&quality));

        // 0.3*0.0 + 0.2*0.0 + 0.2*1.0 + 0.3*0.0 = 0.2
        let quality = SignalQuality {
            ekf_gain: 0.0,
            freshness: 0.0,
            spread_health: 1.0,
            adf_strength: 0.0,
        };
        assert!(!gate.should_execute(&quality));
    }

    #[test]
    fn freshness_computation() {
        let gate = QualityGate::new(0.5, 100_000, 0.01);
        assert!((gate.compute_freshness(0) - 1.0).abs() < 1e-10);
        assert!((gate.compute_freshness(50_000) - 0.5).abs() < 1e-10);
        assert!((gate.compute_freshness(100_000) - 0.0).abs() < 1e-10);
        assert!((gate.compute_freshness(200_000) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn spread_health_computation() {
        let gate = QualityGate::new(0.5, 100_000, 0.01);
        // Normal spread
        assert!((gate.compute_spread_health(0.01) - 1.0).abs() < 1e-10);
        // 1.5x spread -> 2.0 - 1.5 = 0.5
        assert!((gate.compute_spread_health(0.015) - 0.5).abs() < 1e-10);
        // 2x spread -> 0.0
        assert!((gate.compute_spread_health(0.02) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn partial_quality_scores_correctly() {
        let gate = QualityGate::new(0.5, 100_000, 0.01);
        // 0.3*0.5 + 0.2*1.0 + 0.2*0.5 + 0.3*0.5 = 0.15+0.2+0.1+0.15 = 0.6
        let quality = SignalQuality {
            ekf_gain: 0.5,
            freshness: 1.0,
            spread_health: 0.5,
            adf_strength: 0.5,
        };
        let score = gate.evaluate(&quality);
        assert!((score - 0.6).abs() < 1e-10);
        assert!(gate.should_execute(&quality));
    }
}
