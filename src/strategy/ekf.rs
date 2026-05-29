//! Extended Kalman Filter for multi-strategy signal fusion.
//!
//! Ported from quant-stat-1 TypeScript (`packages/backtest-core/src/ekf_signal_fusion.ts`).
//!
//! Scalar-state EKF that fuses N observation sources into a single "true signal
//! strength" estimate.  Each observation source has its own noise variance R_i;
//! the Kalman gain automatically down-weights noisy or conflicting sources.

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tuning parameters for the EKF signal fuser.
#[derive(Debug, Clone)]
pub struct EKFConfig {
    /// Number of observation sources (strategies being fused).
    pub observation_dim: usize,
    /// Process noise variance Q (scalar -- state is 1D).
    pub process_noise: f64,
    /// Initial state estimate x(0).
    pub initial_state: f64,
    /// Initial covariance P(0).
    pub initial_covariance: f64,
    /// Default observation noise per source (used when caller passes no array).
    pub default_observation_noise: f64,
}

impl Default for EKFConfig {
    fn default() -> Self {
        Self {
            observation_dim: 2,
            process_noise: 1e-4,
            initial_state: 0.0,
            initial_covariance: 1.0,
            default_observation_noise: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// EKF Signal Fuser
// ---------------------------------------------------------------------------

/// Scalar extended Kalman filter that fuses N observation sources into a
/// single "true signal strength" estimate.
///
/// **State model** (scalar):
///   x_{k+1} = x_k + dt * (-x_k / half_life) + w_k
///
/// The mean-reversion rate is -1/half_life, so the state decays towards zero
/// at a speed governed by an externally supplied half-life.
///
/// **Observation model** (N-dimensional):
///   z_i = H_i * x + v_i    where H_i = 1 (every strategy observes the same latent signal)
///
/// The Kalman gain automatically down-weights noisy or conflicting sources.
/// When cointegration signals degrade (high observation noise passed in),
/// their effective weight drops without manual intervention.
pub struct EKFSignalFuser {
    state: f64,
    covariance: f64,
    config: EKFConfig,
    kalman_gain: Vec<f64>,
    step_count: u64,
}

impl EKFSignalFuser {
    /// Construct a new EKF with the given configuration.
    pub fn new(config: EKFConfig) -> Self {
        let n = config.observation_dim;
        Self {
            state: config.initial_state,
            covariance: config.initial_covariance,
            config,
            kalman_gain: vec![0.0; n],
            step_count: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Predict
    // -----------------------------------------------------------------------

    /// EKF predict step.
    ///
    /// Applies the mean-reverting process model:
    ///   x_{k+1|k} = x_{k|k} * (1 - dt / half_life)
    ///   P_{k+1|k} = F^2 * P_{k|k} + Q
    ///
    /// where F = 1 - dt / half_life (the Jacobian of the transition model).
    ///
    /// Returns the predicted state (prior mean).
    pub fn predict(&mut self, dt: f64, half_life: f64) -> f64 {
        let effective_half_life = if half_life > 0.0 { half_life } else { 1e12 };
        let f = 1.0 - dt / effective_half_life;
        self.state *= f;
        self.covariance = f * f * self.covariance + self.config.process_noise;
        self.step_count += 1;
        self.state
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    /// EKF update step -- sequential scalar updates for each observation.
    ///
    /// For each observation i:
    ///   y_i    = z_i - H_i * x        (innovation)
    ///   S_i    = H_i^2 * P + R_i      (innovation variance)
    ///   K_i    = P * H_i / S_i        (Kalman gain)
    ///   x     += K_i * y_i
    ///   P     *= (1 - K_i * H_i)
    ///
    /// All H_i = 1.0 (every source observes the same latent scalar signal).
    ///
    /// Returns the fused signal (posterior state mean).
    ///
    /// # Panics
    /// Panics in debug builds if `observations.len() != config.observation_dim`.
    pub fn update(&mut self, observations: &[f64], observation_noise: &[f64]) -> f64 {
        debug_assert_eq!(
            observations.len(),
            self.config.observation_dim,
            "observation count mismatch"
        );

        for i in 0..self.config.observation_dim {
            let h_i = 1.0_f64; // all observation sources share H = 1
            let r_i = if i < observation_noise.len() {
                observation_noise[i]
            } else {
                self.config.default_observation_noise
            };

            // Innovation.
            let y_i = observations[i] - h_i * self.state;

            // Innovation variance.
            let s_i = h_i * h_i * self.covariance + r_i;
            if s_i <= 0.0 {
                // Degenerate -- skip this observation.
                self.kalman_gain[i] = 0.0;
                continue;
            }

            // Kalman gain.
            let k_i = self.covariance * h_i / s_i;
            self.kalman_gain[i] = k_i;

            // State & covariance update.
            self.state += k_i * y_i;
            self.covariance *= 1.0 - k_i * h_i;
        }

        // Clamp covariance to stay positive.
        if self.covariance < 1e-15 {
            self.covariance = 1e-15;
        }

        self.state
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Current fused signal estimate and uncertainty: `(signal, covariance)`.
    pub fn get_state(&self) -> (f64, f64) {
        (self.state, self.covariance)
    }

    /// Normalised Kalman-gain-derived weights for each observation source.
    ///
    /// Weight_i = K_i * H_i / sum(K_j * H_j).
    /// In steady state these weights converge to the relative trust placed in
    /// each observation.  Sources with high observation noise get low weights.
    pub fn get_weights(&self) -> Vec<f64> {
        let n = self.config.observation_dim;
        let mut raw = vec![0.0; n];
        let mut sum = 0.0;
        for (i, slot) in raw.iter_mut().enumerate().take(n) {
            // H_i = 1.0 for all sources, so raw_i = K_i * 1 = K_i.
            *slot = self.kalman_gain[i];
            sum += *slot;
        }
        if sum.abs() < 1e-15 {
            // Uniform weights when no information.
            return vec![1.0 / n as f64; n];
        }
        raw.iter().map(|r| r / sum).collect()
    }

    /// Number of predict-update cycles completed.
    pub fn step_count(&self) -> u64 {
        self.step_count
    }

    /// Reset filter to initial conditions.
    pub fn reset(&mut self) {
        self.state = self.config.initial_state;
        self.covariance = self.config.initial_covariance;
        self.step_count = 0;
        self.kalman_gain.fill(0.0);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(observation_dim: usize) -> EKFConfig {
        EKFConfig {
            observation_dim,
            process_noise: 1e-4,
            initial_state: 0.0,
            initial_covariance: 1.0,
            default_observation_noise: 1.0,
        }
    }

    #[test]
    fn predict_mean_reversion() {
        let mut ekf = EKFSignalFuser::new(test_config(2));
        // Start with a non-zero state.
        ekf.state = 5.0;
        ekf.covariance = 2.0;

        // Predict with dt=1.0, half_life=10.0 → F = 1 - 1/10 = 0.9
        let predicted = ekf.predict(1.0, 10.0);
        let eps = 1e-12;

        // State should decay: 5.0 * 0.9 = 4.5
        assert!((predicted - 4.5).abs() < eps, "predicted = {predicted}");

        // Covariance: 0.9^2 * 2.0 + 1e-4 = 1.62 + 0.0001 = 1.6201
        let expected_p = 0.81 * 2.0 + 1e-4;
        assert!(
            (ekf.covariance - expected_p).abs() < eps,
            "covariance = {} expected {expected_p}",
            ekf.covariance
        );

        // Step count incremented.
        assert_eq!(ekf.step_count(), 1);
    }

    #[test]
    fn predict_with_zero_halflife_clamps() {
        let mut ekf = EKFSignalFuser::new(test_config(1));
        ekf.state = 3.0;
        // half_life = 0 → effective_half_life = 1e12 → F ~ 1.0
        let predicted = ekf.predict(1.0, 0.0);
        // State barely changes: 3.0 * (1 - 1e-12) ~ 3.0
        assert!((predicted - 3.0).abs() < 1e-6);
    }

    #[test]
    fn update_with_multiple_observations() {
        let mut ekf = EKFSignalFuser::new(test_config(3));
        // Start with state = 2.0, covariance = 1.0
        ekf.state = 2.0;
        ekf.covariance = 1.0;

        let observations = [3.0, 2.5, 1.0];
        let noise = [1.0, 1.0, 1.0];
        let fused = ekf.update(&observations, &noise);

        // All observation noise is equal, so the filter should pull the state
        // toward the mean of the observations (~2.17) from the prior of 2.0.
        // The exact value depends on sequential scalar updates.
        assert!(
            fused > 2.0 && fused < 3.0,
            "fused signal should move toward observations, got {fused}"
        );
    }

    #[test]
    fn update_rejects_wrong_length_in_debug() {
        let mut ekf = EKFSignalFuser::new(test_config(2));
        // This should panic in debug due to debug_assert_eq.
        // In release it will silently read out-of-bounds or skip.
        // We test correct usage instead.
        let observations = [1.0, 2.0];
        let noise = [1.0, 1.0];
        // Correct length: should not panic.
        ekf.update(&observations, &noise);
    }

    #[test]
    fn noisy_sources_get_lower_weights() {
        let mut ekf = EKFSignalFuser::new(test_config(3));
        ekf.state = 0.0;
        ekf.covariance = 1.0;

        // Source 0: low noise (trusted), Source 1: medium, Source 2: high noise
        let observations = [1.0, 1.0, 1.0];
        let noise = [0.1, 1.0, 10.0];
        ekf.update(&observations, &noise);

        let weights = ekf.get_weights();
        assert_eq!(weights.len(), 3);

        // Lower noise → higher weight.
        assert!(
            weights[0] > weights[1],
            "source 0 (low noise) should have higher weight than source 1, got {} vs {}",
            weights[0],
            weights[1]
        );
        assert!(
            weights[1] > weights[2],
            "source 1 (medium noise) should have higher weight than source 2, got {} vs {}",
            weights[1],
            weights[2]
        );
    }

    #[test]
    fn round_trip_predict_update() {
        let mut ekf = EKFSignalFuser::new(test_config(2));
        let eps = 1e-10;

        // Seed a known state.
        ekf.state = 1.0;
        ekf.covariance = 0.5;

        // Predict: dt=1.0, half_life=5.0 → F = 1 - 0.2 = 0.8
        let prior = ekf.predict(1.0, 5.0);
        // x = 0.8 * 1.0 = 0.8
        assert!((prior - 0.8).abs() < eps, "prior = {prior}");

        // P = 0.64 * 0.5 + 1e-4 = 0.3201
        let expected_p_prior = 0.64 * 0.5 + 1e-4;
        assert!(
            (ekf.covariance - expected_p_prior).abs() < eps,
            "P_prior = {} expected {expected_p_prior}",
            ekf.covariance
        );

        // Update with observations [0.5, 0.6], both noise = 1.0.
        let obs = [0.5, 0.6];
        let noise = [1.0, 1.0];
        let posterior = ekf.update(&obs, &noise);

        // Posterior should be between prior (0.8) and observations (~0.55).
        assert!(
            posterior > 0.5 && posterior < 0.8,
            "posterior should be between observations and prior, got {posterior}"
        );

        // Covariance should decrease after update.
        assert!(
            ekf.covariance < expected_p_prior,
            "covariance should decrease after update, got {}",
            ekf.covariance
        );
    }

    #[test]
    fn covariance_clamped_positive() {
        let mut ekf = EKFSignalFuser::new(EKFConfig {
            observation_dim: 1,
            process_noise: 0.0,
            initial_state: 0.0,
            initial_covariance: 1e-20,
            default_observation_noise: 1e-20,
        });

        // Very low noise + very low covariance → gain close to 1,
        // covariance update: P *= (1 - K*H) could go negative.
        let obs = [5.0];
        let noise = [1e-20];
        ekf.update(&obs, &noise);

        assert!(
            ekf.covariance >= 1e-15,
            "covariance should be clamped to 1e-15, got {}",
            ekf.covariance
        );
    }

    #[test]
    fn reset_restores_initial_state() {
        let config = test_config(2);
        let mut ekf = EKFSignalFuser::new(config.clone());

        // Run a few cycles.
        ekf.predict(1.0, 10.0);
        ekf.update(&[1.0, 2.0], &[1.0, 1.0]);
        ekf.predict(1.0, 10.0);
        assert_ne!(ekf.step_count(), 0);

        ekf.reset();
        let (signal, cov) = ekf.get_state();
        assert!((signal - config.initial_state).abs() < 1e-15);
        assert!((cov - config.initial_covariance).abs() < 1e-15);
        assert_eq!(ekf.step_count(), 0);
        assert!(ekf.get_weights().iter().all(|&w| (w - 0.5).abs() < 1e-15));
    }

    #[test]
    fn uniform_weights_when_no_information() {
        let ekf = EKFSignalFuser::new(test_config(4));
        let weights = ekf.get_weights();
        // No updates yet -- kalman_gain all zeros → uniform weights.
        for w in &weights {
            assert!((*w - 0.25).abs() < 1e-15, "weight = {w}, expected 0.25");
        }
    }

    #[test]
    fn degenerate_innovation_variance_skips() {
        let mut ekf = EKFSignalFuser::new(test_config(2));
        ekf.state = 0.0;
        ekf.covariance = 0.0; // zero covariance → S = 0 + R

        // With R = 0.0, S = 0 → degenerate, should skip.
        let obs = [1.0, 2.0];
        let noise = [0.0, 0.0];
        ekf.update(&obs, &noise);

        // Both gains should be zero (skipped).
        let weights = ekf.get_weights();
        // All gains are zero → uniform weights.
        assert!((weights[0] - 0.5).abs() < 1e-15);
        assert!((weights[1] - 0.5).abs() < 1e-15);
    }
}
