//! Cointegration trading strategy kernels — ported from quant-stat-1 TypeScript.
//!
//! All statistics computed with stdlib only; no external crates.
//! References:
//!   - quant-stat-1/packages/shared/src/stats.ts (linearRegression, mean, std)
//!   - quant-stat-1/packages/backtest-core/src/event-driven-utils.ts
//!     (adfPvalueApprox, halfLifeBars, computeHybridZScore)

// ---------------------------------------------------------------------------
// Primitive statistics
// ---------------------------------------------------------------------------

/// Arithmetic mean. Returns 0.0 for empty slices (mirrors the TS convention).
fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Population standard deviation. Returns 0.0 for empty slices.
fn pop_std(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let m = mean(xs);
    let variance = xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / xs.len() as f64;
    variance.sqrt()
}

// ---------------------------------------------------------------------------
// Core kernels
// ---------------------------------------------------------------------------

/// Ordinary least-squares regression.
///
/// Returns `(beta, alpha)` where `y = beta * x + alpha`.
/// If the inputs are empty or x has zero variance, returns `(0.0, mean_y)`.
pub fn linear_regression(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return (0.0, 0.0);
    }

    let mx = mean(&xs[..n]);
    let my = mean(&ys[..n]);

    let mut cov = 0.0_f64;
    let mut var_x = 0.0_f64;
    for i in 0..n {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        cov += dx * dy;
        var_x += dx * dx;
    }

    if var_x == 0.0 {
        return (0.0, my);
    }

    let beta = cov / var_x;
    let alpha = my - beta * mx;
    (beta, alpha)
}

/// Augmented Dickey-Fuller test — approximate p-value.
///
/// Computes first differences, regresses Δy on lagged y, then maps the
/// resulting t-statistic through a step-function approximation:
///
///   t < -3.5 → 0.01
///   t < -2.9 → 0.05
///   t < -2.6 → 0.1
///   else     → 0.5
///
/// Returns 0.5 (non-stationary) when the input is shorter than 5 elements
/// or when variance is degenerate.
pub fn adf_test(spread: &[f64]) -> f64 {
    if spread.len() < 5 {
        return 0.5;
    }

    // First differences Δy_t = y_t - y_{t-1}
    let n = spread.len() - 1;
    let diffs: Vec<f64> = (0..n).map(|i| spread[i + 1] - spread[i]).collect();
    let lagged: Vec<f64> = spread[..n].to_vec();

    // Fit Δy_t = alpha + beta * y_{t-1} + epsilon
    let (beta, alpha) = linear_regression(&lagged, &diffs);

    // Residuals
    let residuals: Vec<f64> = (0..n)
        .map(|i| diffs[i] - (alpha + beta * lagged[i]))
        .collect();

    let residual_std = pop_std(&residuals);
    let lag_std = pop_std(&lagged);

    if residual_std == 0.0 || lag_std == 0.0 || n == 0 {
        return 0.5;
    }

    // t = beta / (sigma_eps / (sigma_lag * sqrt(n)))
    let t_stat = beta / (residual_std / (lag_std * (n as f64).sqrt()));

    if t_stat < -3.5 {
        0.01
    } else if t_stat < -2.9 {
        0.05
    } else if t_stat < -2.6 {
        0.1
    } else {
        0.5
    }
}

/// Half-life of mean reversion (in bars).
///
/// Fits an AR(1) model: y_t = phi * y_{t-1} + alpha, then returns
/// `-ln(2) / ln(phi)`.
///
/// `phi` is clamped to the open interval (0, 1). Returns `f64::INFINITY`
/// if the result is not finite or the input is too short.
pub fn half_life(spread: &[f64]) -> f64 {
    if spread.len() < 5 {
        return f64::INFINITY;
    }

    let lagged = &spread[..spread.len() - 1];
    let current = &spread[1..];

    let (phi_raw, _alpha) = linear_regression(lagged, current);

    // Clamp to (-0.9999, 0.9999) then reject anything outside (0, 1)
    let phi = phi_raw.clamp(-0.9999, 0.9999);
    if !phi.is_finite() || phi <= 0.0 || phi >= 1.0 {
        return f64::INFINITY;
    }

    let hl = -2.0_f64.ln() / phi.ln();
    if hl.is_finite() && hl > 0.0 {
        hl
    } else {
        f64::INFINITY
    }
}

/// Standard z-score of the last element relative to the full window.
///
/// Returns 0.0 when the standard deviation is negligible (< 1e-8) or the
/// slice has fewer than 2 elements.
pub fn z_score(history: &[f64]) -> f64 {
    if history.len() < 2 {
        return 0.0;
    }
    let current = history[history.len() - 1];
    let m = mean(history);
    let s = pop_std(history);
    if s < 1e-8 {
        return 0.0;
    }
    (current - m) / s
}

/// Compute the log-price spread series:
///
/// `spread_t = ln(P_a_t) - beta * ln(P_b_t) - alpha`
///
/// The two price slices must have equal length.
pub fn compute_spread(price_a: &[f64], price_b: &[f64], beta: f64, alpha: f64) -> Vec<f64> {
    let n = price_a.len().min(price_b.len());
    (0..n)
        .map(|i| price_a[i].ln() - beta * price_b[i].ln() - alpha)
        .collect()
}

// ---------------------------------------------------------------------------
// Composite signal
// ---------------------------------------------------------------------------

/// Aggregated signal produced by [`evaluate_coint_pair`].
#[derive(Debug, Clone, PartialEq)]
pub struct CointSignal {
    pub z_score: f64,
    pub half_life: f64,
    pub adf_pvalue: f64,
    pub beta: f64,
}

/// End-to-end cointegration evaluation over a rolling window.
///
/// 1. Fits OLS on `ln(P_a)` vs `ln(P_b)` to obtain `(beta, alpha)`.
/// 2. Computes the spread series.
/// 3. Runs an ADF test on the spread.
/// 4. Computes half-life and current z-score.
/// 5. Returns `None` if the ADF p-value exceeds 0.05 or the half-life
///    is non-finite.
///
/// Only the last `window` price observations are used. Requires at least
/// 10 observations.
pub fn evaluate_coint_pair(
    prices_a: &[f64],
    prices_b: &[f64],
    window: usize,
) -> Option<CointSignal> {
    let n = prices_a.len().min(prices_b.len());
    let window = window.min(n);
    if window < 10 {
        return None;
    }

    let start = n - window;
    let a = &prices_a[start..n];
    let b = &prices_b[start..n];

    // Fit spread on log prices
    let log_a: Vec<f64> = a.iter().map(|p| p.ln()).collect();
    let log_b: Vec<f64> = b.iter().map(|p| p.ln()).collect();
    let (beta, alpha) = linear_regression(&log_b, &log_a);

    // Compute spread: ln(P_a) - beta * ln(P_b) - alpha
    let spread = compute_spread(a, b, beta, alpha);

    let adf_pvalue = adf_test(&spread);
    if adf_pvalue > 0.05 {
        return None;
    }

    let hl = half_life(&spread);
    if !hl.is_finite() {
        return None;
    }

    let z = z_score(&spread);

    Some(CointSignal {
        z_score: z,
        half_life: hl,
        adf_pvalue,
        beta,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mean_empty() {
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn test_mean_values() {
        let eps = 1e-12;
        assert!((mean(&[1.0, 2.0, 3.0, 4.0]) - 2.5).abs() < eps);
        assert!((mean(&[10.0]) - 10.0).abs() < eps);
    }

    #[test]
    fn test_pop_std_empty() {
        assert_eq!(pop_std(&[]), 0.0);
    }

    #[test]
    fn test_pop_std_constant() {
        assert!((pop_std(&[5.0, 5.0, 5.0])).abs() < 1e-12);
    }

    #[test]
    fn test_pop_std_values() {
        // population std of [1,2,3] = sqrt(2/3)
        let expected = (2.0_f64 / 3.0).sqrt();
        assert!((pop_std(&[1.0, 2.0, 3.0]) - expected).abs() < 1e-12);
    }

    #[test]
    fn test_linear_regression_perfect() {
        let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys: Vec<f64> = xs.iter().map(|x| 2.0 * x + 3.0).collect();
        let (beta, alpha) = linear_regression(&xs, &ys);
        assert!((beta - 2.0).abs() < 1e-10);
        assert!((alpha - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_linear_regression_empty() {
        let (beta, alpha) = linear_regression(&[], &[]);
        assert_eq!(beta, 0.0);
        assert_eq!(alpha, 0.0);
    }

    #[test]
    fn test_linear_regression_zero_variance() {
        let xs = vec![3.0, 3.0, 3.0];
        let ys = vec![1.0, 2.0, 3.0];
        let (beta, alpha) = linear_regression(&xs, &ys);
        assert_eq!(beta, 0.0);
        assert!((alpha - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_z_score_basic() {
        // values [1,2,3,4,5]: mean=3, pop_std=sqrt(2), current=5
        // z = (5 - 3) / sqrt(2) = 2/sqrt(2) = sqrt(2)
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let z = z_score(&data);
        let expected = 2.0_f64 / 2.0_f64.sqrt();
        assert!((z - expected).abs() < 1e-10);
    }

    #[test]
    fn test_z_score_constant() {
        let data = vec![7.0, 7.0, 7.0, 7.0];
        assert_eq!(z_score(&data), 0.0);
    }

    #[test]
    fn test_z_score_too_short() {
        assert_eq!(z_score(&[42.0]), 0.0);
        assert_eq!(z_score(&[]), 0.0);
    }

    #[test]
    fn test_compute_spread() {
        // Use small values so ln is numerically stable
        let pa = vec![100.0, 200.0];
        let pb = vec![100.0, 200.0];
        let spread = compute_spread(&pa, &pb, 1.0, 0.0);
        // spread_t = ln(P_a) - 1.0*ln(P_b) - 0 = 0
        assert!((spread[0]).abs() < 1e-10);
        assert!((spread[1]).abs() < 1e-10);

        // With offset
        let pa2 = vec![150.0, 300.0];
        let pb2 = vec![100.0, 200.0];
        let spread2 = compute_spread(&pa2, &pb2, 1.0, 0.0);
        // ln(150) - ln(100) = ln(1.5), ln(300) - ln(200) = ln(1.5)
        let expected = 1.5_f64.ln();
        assert!((spread2[0] - expected).abs() < 1e-10);
        assert!((spread2[1] - expected).abs() < 1e-10);
    }

    #[test]
    fn test_adf_test_stationary() {
        // Mean-reverting spread: oscillating around zero
        let spread: Vec<f64> = (0..200)
            .map(|i| {
                let decay = 0.8_f64.powi(i);
                let noise = if i % 3 == 0 { 0.01 } else { -0.01 };
                decay * 5.0 + noise
            })
            .collect();
        let p = adf_test(&spread);
        // Strongly mean-reverting should get a low p-value
        assert!(p <= 0.05, "ADF p-value should be <= 0.05 for mean-reverting series, got {p}");
    }

    #[test]
    fn test_adf_test_random_walk() {
        // Random walk should not be stationary
        let mut rw = vec![0.0_f64; 200];
        for i in 1..200 {
            rw[i] = rw[i - 1] + 1.0; // deterministic trend
        }
        let p = adf_test(&rw);
        assert!(
            p > 0.05,
            "ADF p-value should be > 0.05 for trending series, got {p}"
        );
    }

    #[test]
    fn test_adf_test_too_short() {
        assert_eq!(adf_test(&[1.0, 2.0, 3.0]), 0.5);
    }

    #[test]
    fn test_half_life_too_short() {
        assert_eq!(half_life(&[1.0, 2.0, 3.0]), f64::INFINITY);
    }

    #[test]
    fn test_half_life_mean_reverting() {
        // AR(1) with phi = 0.5 → half-life = -ln(2)/ln(0.5) = 1.0
        let mut series = vec![0.0_f64; 100];
        series[0] = 5.0;
        for i in 1..100 {
            series[i] = 0.5 * series[i - 1];
        }
        let hl = half_life(&series);
        assert!(
            (hl - 1.0).abs() < 0.05,
            "Half-life should be ~1.0, got {hl}"
        );
    }

    #[test]
    fn test_evaluate_coint_pair_cointegrated() {
        // Build two cointegrated price series:
        // A_t = 100 + noise, B_t = 50 + noise, with strong mean-reverting spread
        let n = 200usize;
        let prices_a: Vec<f64> = (0..n)
            .map(|i| 100.0 + 0.5 * (i as f64 * 0.3).sin() + 0.001 * (i as f64))
            .collect();
        let prices_b: Vec<f64> = (0..n)
            .map(|i| 50.0 + 0.25 * (i as f64 * 0.3).sin() + 0.0005 * (i as f64))
            .collect();

        let result = evaluate_coint_pair(&prices_a, &prices_b, 200);
        assert!(result.is_some(), "Cointegrated pair should produce a signal");
        let sig = result.unwrap();
        assert!(sig.adf_pvalue <= 0.05);
        assert!(sig.half_life.is_finite());
        assert!(sig.half_life > 0.0);
    }

    #[test]
    fn test_evaluate_coint_pair_window_too_small() {
        let a = vec![100.0; 5];
        let b = vec![50.0; 5];
        assert!(evaluate_coint_pair(&a, &b, 5).is_none());
    }
}
