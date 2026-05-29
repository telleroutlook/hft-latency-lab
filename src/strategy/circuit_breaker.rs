//! Trading circuit breaker -- halts strategy when safety conditions trigger.

/// Trading circuit breaker -- halts strategy when safety conditions trigger.
///
/// Monitors cumulative drawdown, trade rate, and consecutive losses.
/// When any threshold is breached, trading is suspended for a cooldown period.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    max_drawdown: f64,
    max_trades_per_sec: f64,
    max_consecutive_losses: u32,
    cooldown_us: u64,

    cumulative_pnl: f64,
    trade_timestamps: Vec<u64>,
    consecutive_losses: u32,
    triggered: bool,
    trigger_time: u64,
}

impl CircuitBreaker {
    pub fn new(
        max_drawdown: f64,
        max_trades_per_sec: f64,
        max_consecutive_losses: u32,
        cooldown_us: u64,
    ) -> Self {
        Self {
            max_drawdown,
            max_trades_per_sec,
            max_consecutive_losses,
            cooldown_us,
            cumulative_pnl: 0.0,
            trade_timestamps: Vec::new(),
            consecutive_losses: 0,
            triggered: false,
            trigger_time: 0,
        }
    }

    /// Permissive defaults that never trigger -- safe for existing pipelines.
    pub fn permissive() -> Self {
        Self::new(f64::INFINITY, f64::INFINITY, u32::MAX, 0)
    }

    /// Check if trading is allowed at `now_us`.
    ///
    /// Returns `false` when the breaker is in cooldown; `true` otherwise.
    /// A triggered breaker auto-resets once the cooldown elapses.
    pub fn is_active(&mut self, now_us: u64) -> bool {
        if !self.triggered {
            return true;
        }
        if now_us >= self.trigger_time + self.cooldown_us {
            self.reset();
            return true;
        }
        false
    }

    /// Record a trade result. Call after each trade completes.
    pub fn record_trade(&mut self, pnl: f64, timestamp_us: u64) {
        self.cumulative_pnl += pnl;

        if pnl < 0.0 {
            self.consecutive_losses += 1;
        } else {
            self.consecutive_losses = 0;
        }

        // Rate-limit bookkeeping: keep only timestamps within the last 1 second.
        let cutoff = timestamp_us.saturating_sub(1_000_000);
        self.trade_timestamps.retain(|&t| t >= cutoff);
        self.trade_timestamps.push(timestamp_us);

        // Check all trigger conditions.
        if self.cumulative_pnl.abs() >= self.max_drawdown && self.cumulative_pnl < 0.0 {
            self.trigger(timestamp_us);
        }
        if self.trade_timestamps.len() as f64 > self.max_trades_per_sec {
            self.trigger(timestamp_us);
        }
        if self.consecutive_losses >= self.max_consecutive_losses {
            self.trigger(timestamp_us);
        }
    }

    /// Manually reset the circuit breaker.
    pub fn reset(&mut self) {
        self.triggered = false;
        self.trigger_time = 0;
        self.consecutive_losses = 0;
        self.trade_timestamps.clear();
        // Note: cumulative_pnl is NOT reset -- drawdown is cumulative across
        // the strategy's lifetime unless the caller explicitly handles it.
    }

    fn trigger(&mut self, timestamp_us: u64) {
        self.triggered = true;
        self.trigger_time = timestamp_us;
    }

    /// Whether the breaker is currently in a triggered state.
    pub fn is_triggered(&self) -> bool {
        self.triggered
    }

    /// Current cumulative PnL tracked by the breaker.
    pub fn cumulative_pnl(&self) -> f64 {
        self.cumulative_pnl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissive_never_triggers() {
        let mut cb = CircuitBreaker::permissive();
        // Record many consecutive losses.
        for _ in 0..1000 {
            cb.record_trade(-100.0, 1_000_000);
        }
        assert!(cb.is_active(2_000_000));
        assert!(!cb.is_triggered());
    }

    #[test]
    fn drawdown_trigger() {
        let mut cb = CircuitBreaker::new(500.0, f64::INFINITY, u32::MAX, 1_000_000);
        cb.record_trade(-200.0, 100);
        assert!(cb.is_active(200));
        assert!(!cb.is_triggered());

        cb.record_trade(-400.0, 200);
        assert!(cb.is_triggered());
        // Total drawdown: 600 >= 500
        assert!(!cb.is_active(300));
    }

    #[test]
    fn rate_limit_trigger() {
        let mut cb = CircuitBreaker::new(f64::INFINITY, 5.0, u32::MAX, 1_000_000);
        // 5 trades in 1 second should be fine.
        for i in 0..5 {
            cb.record_trade(1.0, 100_000 + i * 100_000);
        }
        assert!(!cb.is_triggered());

        // 6th trade in the same 1-second window triggers.
        cb.record_trade(1.0, 150_000);
        assert!(cb.is_triggered());
    }

    #[test]
    fn consecutive_losses_trigger() {
        let mut cb = CircuitBreaker::new(f64::INFINITY, f64::INFINITY, 3, 500_000);
        cb.record_trade(-10.0, 100);
        cb.record_trade(-10.0, 200);
        assert!(!cb.is_triggered());

        cb.record_trade(-10.0, 300);
        assert!(cb.is_triggered());
    }

    #[test]
    fn winning_trade_resets_consecutive_losses() {
        let mut cb = CircuitBreaker::new(f64::INFINITY, f64::INFINITY, 3, 500_000);
        cb.record_trade(-10.0, 100);
        cb.record_trade(-10.0, 200);
        cb.record_trade(50.0, 300); // win resets counter
        cb.record_trade(-10.0, 400);
        cb.record_trade(-10.0, 500);
        assert!(!cb.is_triggered()); // only 2 consecutive losses
    }

    #[test]
    fn cooldown_auto_resets() {
        let mut cb = CircuitBreaker::new(f64::INFINITY, f64::INFINITY, 1, 1_000_000);
        cb.record_trade(-10.0, 100);
        assert!(cb.is_triggered());
        assert!(!cb.is_active(500)); // still in cooldown

        // After cooldown elapses, is_active auto-resets.
        assert!(cb.is_active(1_000_100));
        assert!(!cb.is_triggered());
    }

    #[test]
    fn manual_reset_clears_state() {
        let mut cb = CircuitBreaker::new(100.0, f64::INFINITY, u32::MAX, 1_000_000);
        cb.record_trade(-200.0, 100);
        assert!(cb.is_triggered());

        cb.reset();
        assert!(!cb.is_triggered());
        assert!(cb.is_active(200));
    }

    #[test]
    fn rate_limit_window_slides() {
        let mut cb = CircuitBreaker::new(f64::INFINITY, 3.0, u32::MAX, 500_000);
        // 3 trades at t=100, 200, 300
        cb.record_trade(1.0, 100);
        cb.record_trade(1.0, 200);
        cb.record_trade(1.0, 300);
        assert!(!cb.is_triggered());

        // At t=1_000_101 the cutoff is 101, so t=100 slides out of the 1s window.
        // Only t=200 and t=300 remain in-window. Adding one more gives 3 total,
        // which equals max_trades_per_sec (3.0) and should NOT trigger (> is strict).
        cb.record_trade(1.0, 1_000_101);
        assert!(!cb.is_triggered());
    }

    #[test]
    fn drawdown_only_triggers_on_negative_pnl() {
        let mut cb = CircuitBreaker::new(500.0, f64::INFINITY, u32::MAX, 1_000_000);
        // Win 600 -- cumulative is +600, not negative, should not trigger.
        cb.record_trade(600.0, 100);
        assert!(!cb.is_triggered());
    }
}
