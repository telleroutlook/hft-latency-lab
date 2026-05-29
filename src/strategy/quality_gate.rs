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

/// Weights for quality dimensions with adaptive learning.
#[derive(Debug, Clone)]
pub struct QualityWeights {
    pub gain: f64,
    pub freshness: f64,
    pub spread: f64,
    pub adf: f64,
    learning_rate: f64,
    profitability: [f64; 4],
}

impl Default for QualityWeights {
    fn default() -> Self {
        Self {
            gain: 0.3,
            freshness: 0.2,
            spread: 0.2,
            adf: 0.3,
            learning_rate: 0.05,
            profitability: [0.0; 4],
        }
    }
}

impl QualityWeights {
    /// Create weights with a custom EMA learning rate.
    pub fn with_learning_rate(mut self, lr: f64) -> Self {
        self.learning_rate = lr;
        self
    }

    /// Update per-dimension profitability tracking and adjust weights.
    ///
    /// If a dimension's value was high AND the trade was profitable, that
    /// dimension's weight increases. If high but losing, the weight decreases.
    /// EMA smoothing prevents oscillation. Weights are normalized to sum to 1.0.
    pub fn update_weights(&mut self, quality: &SignalQuality, trade_profitable: bool) {
        let dims = [
            quality.ekf_gain,
            quality.freshness,
            quality.spread_health,
            quality.adf_strength,
        ];
        let reward = if trade_profitable { 1.0 } else { -1.0 };

        for (i, &dim_val) in dims.iter().enumerate() {
            let signal = dim_val * reward;
            self.profitability[i] =
                self.profitability[i] * (1.0 - self.learning_rate) + signal * self.learning_rate;
        }

        let weights = &mut [
            &mut self.gain,
            &mut self.freshness,
            &mut self.spread,
            &mut self.adf,
        ];
        for (i, w) in weights.iter_mut().enumerate() {
            **w = (**w + self.learning_rate * self.profitability[i]).max(0.01);
        }

        let sum = self.gain + self.freshness + self.spread + self.adf;
        if sum > 0.0 {
            self.gain /= sum;
            self.freshness /= sum;
            self.spread /= sum;
            self.adf /= sum;
        }
    }
}

/// Evaluates signal quality and gates trade execution.
#[derive(Debug, Clone)]
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

    #[test]
    fn weights_converge_toward_predictive_dimension() {
        let mut gate = QualityGate::new(0.5, 100_000, 0.01);
        // Use a higher learning rate so convergence is visible in few iterations.
        gate.weights = QualityWeights::default().with_learning_rate(0.2);

        let initial_adf = gate.weights.adf;
        let initial_gain = gate.weights.gain;

        // Simulate many trades where adf_strength is predictive (high when
        // profitable, low when not) but gain is anti-predictive.
        for i in 0..200 {
            let profitable = i % 2 == 0;
            let quality = if profitable {
                SignalQuality {
                    ekf_gain: 0.1,
                    freshness: 0.5,
                    spread_health: 0.5,
                    adf_strength: 0.95,
                }
            } else {
                SignalQuality {
                    ekf_gain: 0.9,
                    freshness: 0.5,
                    spread_health: 0.5,
                    adf_strength: 0.05,
                }
            };
            gate.weights.update_weights(&quality, profitable);
        }

        // adf weight should have grown, gain weight should have shrunk.
        assert!(
            gate.weights.adf > initial_adf,
            "adf weight should increase: got {} vs initial {}",
            gate.weights.adf,
            initial_adf
        );
        assert!(
            gate.weights.gain < initial_gain,
            "gain weight should decrease: got {} vs initial {}",
            gate.weights.gain,
            initial_gain
        );

        // Weights must still sum to 1.0.
        let sum =
            gate.weights.gain + gate.weights.freshness + gate.weights.spread + gate.weights.adf;
        assert!((sum - 1.0).abs() < 1e-10, "weights sum = {sum}");
    }
}
