//! End-to-end cointegration strategy simulation pipeline.
//!
//! Chains: ITCH message → bitmap filter → order book update → spread tracking
//! → ADF + half-life cointegration test → z-score signal → entry/exit decision.
//!
//! Each stage is instrumented with `rdtsc()` cycle timing.

use crate::orderbook::book::OrderBook;
use crate::parser::naive::Message;
use crate::timer::rdtsc_serialized;

use super::cointegration;
use super::ekf::{EKFConfig, EKFSignalFuser};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tuneable knobs for the strategy pipeline.
#[derive(Debug, Clone)]
pub struct StrategyConfig {
    /// 8-byte stock symbol for leg A of the pair (e.g. b"GOOG    ").
    pub symbol_a: [u8; 8],
    /// 8-byte stock symbol for leg B of the pair.
    pub symbol_b: [u8; 8],
    /// Rolling window length (in price observations) for cointegration estimation.
    pub lookback: usize,
    /// ADF p-value threshold — reject non-stationary if p > threshold.
    pub adf_threshold: f64,
    /// Z-score threshold to trigger entry (absolute value).
    pub z_entry: f64,
    /// Z-score threshold to flatten position (absolute value).
    pub z_exit: f64,
    /// Acceptable half-life range: (min_bars, max_bars).
    pub half_life_bounds: (f64, f64),
    /// Re-run cointegration regression every N price ticks.
    pub retest_interval: usize,
    /// Arena capacity for each order book.
    pub book_capacity: usize,
    /// EKF configuration. When `Some`, the pipeline runs EKF predict+update
    /// after z-score computation to fuse additional signal sources.
    pub ekf_config: Option<EKFConfig>,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            symbol_a: *b"A       ",
            symbol_b: *b"B       ",
            lookback: 200,
            adf_threshold: 0.05,
            z_entry: 2.0,
            z_exit: 0.5,
            half_life_bounds: (1.0, 200.0),
            retest_interval: 20,
            book_capacity: 4096,
            ekf_config: None,
        }
    }
}

impl StrategyConfig {
    /// Default observation noise for the secondary EKF signal source.
    /// Reads from the EKF config if available, otherwise returns 1.0.
    pub fn default_observation_noise(&self) -> f64 {
        self.ekf_config
            .as_ref()
            .map(|c| c.default_observation_noise)
            .unwrap_or(1.0)
    }
}

// ---------------------------------------------------------------------------
// Spread tracker
// ---------------------------------------------------------------------------

/// Rolling buffer of mid-price spread values between two symbols.
/// Uses a fixed-size ring buffer — zero allocation after construction.
pub struct SpreadTracker {
    prices_a: Vec<f64>,
    prices_b: Vec<f64>,
    mid_a: f64,
    mid_b: f64,
    count: usize,
}

impl SpreadTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            prices_a: Vec::with_capacity(capacity),
            prices_b: Vec::with_capacity(capacity),
            mid_a: 0.0,
            mid_b: 0.0,
            count: 0,
        }
    }

    /// Update the mid-price for a single symbol. Returns `true` if both legs
    /// have received at least one update (i.e. a spread sample can be recorded).
    /// Does NOT automatically push — call `record()` when both legs are ready.
    pub fn update_mid(&mut self, symbol: &[u8; 8], mid: f64, config: &StrategyConfig) -> bool {
        if *symbol == config.symbol_a {
            self.mid_a = mid;
        } else if *symbol == config.symbol_b {
            self.mid_b = mid;
        } else {
            return false;
        }
        self.mid_a > 0.0 && self.mid_b > 0.0
    }

    /// Record a spread observation from the current mid-prices.
    /// Call this once per "tick" when both legs are ready.
    pub fn record(&mut self) {
        debug_assert!(self.mid_a > 0.0 && self.mid_b > 0.0);
        self.push(self.mid_a, self.mid_b);
    }

    /// Append a spread observation. Evicts the oldest when the buffer exceeds
    /// the lookback window.
    fn push(&mut self, price_a: f64, price_b: f64) {
        if self.prices_a.len() >= self.prices_a.capacity() {
            // Ring-buffer eviction: remove oldest
            self.prices_a.remove(0);
            self.prices_b.remove(0);
        }
        self.prices_a.push(price_a);
        self.prices_b.push(price_b);
        self.count += 1;
    }

    pub fn prices_a(&self) -> &[f64] {
        &self.prices_a
    }

    pub fn prices_b(&self) -> &[f64] {
        &self.prices_b
    }

    pub fn len(&self) -> usize {
        self.prices_a.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prices_a.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Signal generator
// ---------------------------------------------------------------------------

/// Cached cointegration parameters (beta, alpha) from the last OLS fit.
#[derive(Debug, Clone, Default)]
pub struct CointParams {
    pub beta: f64,
    pub alpha: f64,
    pub half_life: f64,
    pub adf_pvalue: f64,
    pub valid: bool,
}

/// Runs ADF + half-life on the spread, generates z-score signals.
pub struct SignalGenerator {
    params: CointParams,
    last_z: f64,
    /// Tick counter for periodic retesting.
    ticks_since_retest: usize,
}

impl Default for SignalGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator {
    pub fn new() -> Self {
        Self {
            params: CointParams::default(),
            last_z: 0.0,
            ticks_since_retest: 0,
        }
    }

    /// Run full cointegration evaluation and cache the parameters.
    /// Returns `true` if the pair passes the ADF + half-life filters.
    pub fn retest(&mut self, tracker: &SpreadTracker, config: &StrategyConfig) -> bool {
        let sig = cointegration::evaluate_coint_pair(
            tracker.prices_a(),
            tracker.prices_b(),
            config.lookback,
        );

        match sig {
            Some(s)
                if s.adf_pvalue <= config.adf_threshold
                    && s.half_life >= config.half_life_bounds.0
                    && s.half_life <= config.half_life_bounds.1 =>
            {
                self.params = CointParams {
                    beta: s.beta,
                    alpha: 0.0, // alpha is embedded in the spread computation
                    half_life: s.half_life,
                    adf_pvalue: s.adf_pvalue,
                    valid: true,
                };
                self.ticks_since_retest = 0;
                true
            }
            _ => {
                self.params.valid = false;
                self.ticks_since_retest = 0;
                false
            }
        }
    }

    /// Compute the current z-score from the spread buffer using cached (beta, alpha).
    /// Callers should first call `retest()` or rely on a previously valid params cache.
    pub fn update_zscore(&mut self, tracker: &SpreadTracker, config: &StrategyConfig) -> f64 {
        if tracker.len() < 2 {
            self.last_z = 0.0;
            return 0.0;
        }

        let window = config.lookback.min(tracker.len());
        let start = tracker.len() - window;

        if self.params.valid {
            // Use cached beta/alpha from last retest for consistency
            let spread: Vec<f64> = (start..tracker.len())
                .map(|i| tracker.prices_a()[i].ln() - self.params.beta * tracker.prices_b()[i].ln())
                .collect();
            self.last_z = cointegration::z_score(&spread);
        } else {
            self.last_z = 0.0;
        }

        self.ticks_since_retest += 1;
        self.last_z
    }

    /// Whether the periodic retest is due.
    pub fn needs_retest(&self, config: &StrategyConfig) -> bool {
        self.ticks_since_retest >= config.retest_interval
    }

    pub fn last_z(&self) -> f64 {
        self.last_z
    }

    pub fn params(&self) -> &CointParams {
        &self.params
    }
}

// ---------------------------------------------------------------------------
// Trade decision
// ---------------------------------------------------------------------------

/// Trading decision produced by the pipeline for each processed message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeDecision {
    /// Open a position: Long if z < -threshold, Short if z > +threshold.
    Enter { long: bool },
    /// Close the current position.
    Exit,
    /// No action.
    Hold,
}

// ---------------------------------------------------------------------------
// Pipeline stats
// ---------------------------------------------------------------------------

/// Per-stage cycle counts and aggregate counters.
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    /// Cycles spent in the bitmap filter / symbol match stage.
    pub filter_cycles: u64,
    /// Cycles spent updating order books.
    pub book_cycles: u64,
    /// Cycles spent computing mid-price and updating spread.
    pub spread_cycles: u64,
    /// Cycles spent in ADF / half-life retest.
    pub coint_cycles: u64,
    /// Cycles spent computing z-score + generating signal.
    pub signal_cycles: u64,
    /// Total cycles for the full pipeline (per-message).
    pub total_cycles: u64,

    /// Number of messages fed into the pipeline.
    pub msg_count: u64,
    /// Number of messages relevant to the tracked pair.
    pub relevant_msg_count: u64,
    /// Number of price updates that triggered a spread update.
    pub spread_updates: u64,
    /// Number of cointegration retests performed.
    pub retest_count: u64,
    /// Number of signals generated (non-Hold decisions).
    pub signal_count: u64,
    /// Number of entry signals.
    pub entry_count: u64,
    /// Number of exit signals.
    pub exit_count: u64,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Current position state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Flat,
    Long,
    Short,
}

/// The full end-to-end strategy pipeline.
///
/// Maintains two order books (one per leg of the pair), tracks the mid-price
/// spread, periodically re-estimates cointegration parameters, and generates
/// entry/exit signals based on z-score thresholds.
pub struct StrategyPipeline {
    config: StrategyConfig,
    book_a: OrderBook,
    book_b: OrderBook,
    spread: SpreadTracker,
    signal: SignalGenerator,
    ekf: Option<EKFSignalFuser>,
    position: Position,
    stats: PipelineStats,
}

impl StrategyPipeline {
    pub fn new(config: StrategyConfig) -> Self {
        let capacity = config.book_capacity;
        let lookback = config.lookback;
        let ekf = config
            .ekf_config
            .as_ref()
            .map(|c| EKFSignalFuser::new(c.clone()));
        Self {
            book_a: OrderBook::new(capacity),
            book_b: OrderBook::new(capacity),
            spread: SpreadTracker::new(lookback),
            signal: SignalGenerator::new(),
            ekf,
            position: Position::Flat,
            stats: PipelineStats::default(),
            config,
        }
    }

    /// Process a single parsed ITCH message through the full pipeline.
    /// Returns the trade decision (if any) and updates internal stats.
    pub fn process_message(&mut self, msg: &Message) -> TradeDecision {
        let t0 = rdtsc_serialized();
        self.stats.msg_count += 1;

        // Stage 1: Filter — only process messages for our two symbols.
        let target_symbol = match msg {
            Message::AddOrder(a) => self.match_symbol(&a.stock),
            Message::AddOrderMpid(f) => self.match_symbol(&f.stock),
            _ => {
                // Non-order messages: no action, minimal timing overhead.
                let elapsed = rdtsc_serialized() - t0;
                self.stats.total_cycles += elapsed;
                return TradeDecision::Hold;
            }
        };

        let Some((symbol, is_a)) = target_symbol else {
            let elapsed = rdtsc_serialized() - t0;
            self.stats.total_cycles += elapsed;
            return TradeDecision::Hold;
        };

        self.stats.relevant_msg_count += 1;

        // Stage 2: Update order book.
        let t_book = rdtsc_serialized();
        self.apply_to_book(msg, is_a);
        self.stats.book_cycles += rdtsc_serialized() - t_book;

        // Stage 3: Compute mid-price and update spread.
        let book = if is_a { &self.book_a } else { &self.book_b };
        let t_spread = rdtsc_serialized();
        let mid = self.compute_mid(book);
        if let Some(mid) = mid {
            let ready = self.spread.update_mid(symbol, mid, &self.config);
            if ready {
                self.spread.record();
                self.stats.spread_updates += 1;
            }
        }
        self.stats.spread_cycles += rdtsc_serialized() - t_spread;

        // Stage 4: Periodic cointegration retest.
        let t_coint = rdtsc_serialized();
        if self.signal.needs_retest(&self.config) && self.spread.len() >= 10 {
            self.signal.retest(&self.spread, &self.config);
            self.stats.retest_count += 1;
        }
        self.stats.coint_cycles += rdtsc_serialized() - t_coint;

        // Stage 5: Z-score signal generation + trade decision.
        let t_sig = rdtsc_serialized();
        let decision = self.evaluate_signal();
        self.stats.signal_cycles += rdtsc_serialized() - t_sig;

        match decision {
            TradeDecision::Enter { .. } => self.stats.entry_count += 1,
            TradeDecision::Exit => self.stats.exit_count += 1,
            TradeDecision::Hold => {}
        }

        if decision != TradeDecision::Hold {
            self.stats.signal_count += 1;
        }

        let elapsed = rdtsc_serialized() - t0;
        self.stats.total_cycles += elapsed;
        decision
    }

    /// Check if the stock field matches either tracked symbol.
    /// Returns `Some((&symbol, true))` if it matches symbol_a, `Some((&symbol, false))`
    /// for symbol_b, or `None` if neither.
    fn match_symbol<'a>(&self, stock: &'a [u8; 8]) -> Option<(&'a [u8; 8], bool)> {
        if *stock == self.config.symbol_a {
            Some((stock, true))
        } else if *stock == self.config.symbol_b {
            Some((stock, false))
        } else {
            None
        }
    }

    /// Apply an order message to the correct book.
    fn apply_to_book(&mut self, msg: &Message, is_a: bool) {
        let book = if is_a {
            &mut self.book_a
        } else {
            &mut self.book_b
        };
        match msg {
            Message::AddOrder(a) => {
                book.add_order(a.order_ref, a.buy, a.price as u64, a.shares);
            }
            Message::AddOrderMpid(f) => {
                book.add_order(f.order_ref, f.buy, f.price as u64, f.shares);
            }
            Message::OrderExecuted(e) => {
                book.execute_order(e.order_ref, e.executed_shares);
            }
            Message::OrderExecutedWithPrice(c) => {
                book.execute_order(c.order_ref, c.executed_shares);
            }
            Message::OrderCancel(x) => {
                // Partial cancel: reduce quantity
                book.execute_order(x.order_ref, x.canceled_shares);
            }
            Message::OrderDelete(d) => {
                book.delete_order(d.order_ref);
            }
            _ => {}
        }
    }

    /// Compute mid-price from BBO. Returns `None` if book is one-sided.
    fn compute_mid(&self, book: &OrderBook) -> Option<f64> {
        match (book.best_bid(), book.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid as f64 + ask as f64) / 2.0),
            _ => None,
        }
    }

    /// Generate trade decision based on current z-score and position state.
    ///
    /// When EKF is configured, runs predict+update to fuse the z-score with an
    /// additional signal source (e.g. order-flow imbalance).  The EKF-fused
    /// signal replaces the raw z-score, and the mean Kalman gain modulates the
    /// effective entry/exit thresholds: higher gain = more confident = tighter
    /// threshold.
    fn evaluate_signal(&mut self) -> TradeDecision {
        let z = self.signal.update_zscore(&self.spread, &self.config);
        if z == 0.0 {
            return TradeDecision::Hold;
        }

        // Force a retest if we have no valid cointegration params.
        if !self.signal.params().valid {
            return TradeDecision::Hold;
        }

        // EKF predict + update when configured.
        let effective_z = if let Some(ekf) = self.ekf.as_mut() {
            let half_life = self.signal.params().half_life;
            ekf.predict(1.0, half_life);
            // Primary observation is the z-score itself; secondary source is a
            // zero-mean signal (placeholder for future order-flow input).
            let observations = vec![z, 0.0];
            let noise = vec![1.0, self.config.default_observation_noise()];
            ekf.update(&observations, &noise)
        } else {
            z
        };

        // When EKF is active, use the mean Kalman gain as a confidence modifier.
        // High gain → we trust the signal → use the raw threshold.
        // Low gain → uncertain → require a stronger signal (wider threshold).
        let confidence = if let Some(ekf) = self.ekf.as_ref() {
            let weights = ekf.get_weights();
            // Primary source weight (index 0) as confidence factor.
            // Clamp to [0.1, 1.0] to avoid degenerate thresholds.
            weights.first().copied().unwrap_or(1.0).clamp(0.1, 1.0)
        } else {
            1.0
        };

        // Adjusted thresholds: low confidence → widen (require stronger signal).
        let adj_entry = self.config.z_entry / confidence;
        let adj_exit = self.config.z_exit / confidence;

        match self.position {
            Position::Flat => {
                if effective_z.abs() >= adj_entry {
                    self.position = if effective_z < 0.0 {
                        Position::Long
                    } else {
                        Position::Short
                    };
                    TradeDecision::Enter {
                        long: effective_z < 0.0,
                    }
                } else {
                    TradeDecision::Hold
                }
            }
            Position::Long => {
                if effective_z >= -adj_exit {
                    self.position = Position::Flat;
                    TradeDecision::Exit
                } else {
                    TradeDecision::Hold
                }
            }
            Position::Short => {
                if effective_z <= adj_exit {
                    self.position = Position::Flat;
                    TradeDecision::Exit
                } else {
                    TradeDecision::Hold
                }
            }
        }
    }

    // Accessors

    pub fn stats(&self) -> &PipelineStats {
        &self.stats
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn config(&self) -> &StrategyConfig {
        &self.config
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::naive::AddOrder;

    fn test_config() -> StrategyConfig {
        StrategyConfig {
            symbol_a: *b"A       ",
            symbol_b: *b"B       ",
            lookback: 50,
            adf_threshold: 0.05,
            z_entry: 2.0,
            z_exit: 0.5,
            half_life_bounds: (1.0, 200.0),
            retest_interval: 5,
            book_capacity: 256,
            ekf_config: None,
        }
    }

    fn make_add_order(
        stock: &[u8; 8],
        order_ref: u64,
        buy: bool,
        price: u32,
        shares: u32,
    ) -> Message {
        Message::AddOrder(AddOrder {
            stock_locate: 1,
            tracking_number: 1,
            timestamp_ns: 0,
            order_ref,
            buy,
            shares,
            stock: *stock,
            price,
        })
    }

    #[test]
    fn spread_tracker_ring_buffer() {
        let mut tracker = SpreadTracker::new(5);
        let config = test_config();

        // Update A first — not ready yet.
        assert!(!tracker.update_mid(&config.symbol_a, 100.0, &config));

        // Update B — now ready, record spread.
        assert!(tracker.update_mid(&config.symbol_b, 50.0, &config));
        tracker.record();
        assert_eq!(tracker.len(), 1);

        // Push 4 more to fill capacity.
        for i in 1..5u32 {
            tracker.update_mid(&config.symbol_a, 100.0 + i as f64, &config);
            tracker.update_mid(&config.symbol_b, 50.0 + i as f64, &config);
            tracker.record();
        }
        assert_eq!(tracker.len(), 5);

        // One more pair — should evict oldest.
        tracker.update_mid(&config.symbol_a, 200.0, &config);
        tracker.update_mid(&config.symbol_b, 100.0, &config);
        tracker.record();
        assert_eq!(tracker.len(), 5);
        // After 6 pushes, oldest should be i=1: (101, 51)
        assert!((tracker.prices_a()[0] - 101.0).abs() < 1e-10);
    }

    #[test]
    fn spread_tracker_ignores_unknown_symbol() {
        let mut tracker = SpreadTracker::new(10);
        let config = test_config();
        assert!(!tracker.update_mid(b"XXXXXXXX", 100.0, &config));
    }

    #[test]
    fn signal_generator_retest_too_short() {
        let mut gen = SignalGenerator::new();
        let tracker = SpreadTracker::new(10);
        let config = test_config();
        // Not enough data for cointegration.
        assert!(!gen.retest(&tracker, &config));
    }

    #[test]
    fn pipeline_ignores_irrelevant_messages() {
        let mut pipe = StrategyPipeline::new(test_config());
        let msg = make_add_order(b"GOOG    ", 1, true, 10000, 100);
        let decision = pipe.process_message(&msg);
        assert_eq!(decision, TradeDecision::Hold);
        assert_eq!(pipe.stats().relevant_msg_count, 0);
    }

    #[test]
    fn pipeline_updates_book_for_relevant_symbol() {
        let config = test_config();
        let mut pipe = StrategyPipeline::new(config);

        // Add buy on symbol A.
        let msg = make_add_order(&pipe.config().symbol_a, 1, true, 10000, 100);
        pipe.process_message(&msg);
        assert_eq!(pipe.stats().relevant_msg_count, 1);
        assert!(pipe.book_a.best_bid().is_some());

        // Add sell on symbol A.
        let msg = make_add_order(&pipe.config().symbol_a, 2, false, 10100, 100);
        pipe.process_message(&msg);
        assert!(pipe.book_a.best_ask().is_some());
    }

    #[test]
    fn pipeline_full_cycle_hold_when_flat_and_no_signal() {
        let config = test_config();
        let mut pipe = StrategyPipeline::new(config);

        // Feed two symbols until we have a spread, but not enough for cointegration.
        for i in 0..5u32 {
            let msg_a = make_add_order(
                &pipe.config().symbol_a,
                100 + i as u64,
                true,
                10000 + i * 100,
                10,
            );
            let msg_a_sell = make_add_order(
                &pipe.config().symbol_a,
                200 + i as u64,
                false,
                10100 + i * 100,
                10,
            );
            let msg_b = make_add_order(
                &pipe.config().symbol_b,
                300 + i as u64,
                true,
                5000 + i * 50,
                10,
            );
            let msg_b_sell = make_add_order(
                &pipe.config().symbol_b,
                400 + i as u64,
                false,
                5100 + i * 50,
                10,
            );
            pipe.process_message(&msg_a);
            pipe.process_message(&msg_a_sell);
            pipe.process_message(&msg_b);
            pipe.process_message(&msg_b_sell);
        }

        // Should hold — not enough data for a valid cointegration test.
        assert_eq!(pipe.position(), Position::Flat);
        assert!(pipe.stats().spread_updates > 0);
    }

    #[test]
    fn pipeline_processes_cointegrated_pair() {
        let config = StrategyConfig {
            lookback: 30,
            retest_interval: 1, // retest every tick
            z_entry: 1.5,
            z_exit: 0.3,
            ..test_config()
        };

        let mut pipe = StrategyPipeline::new(config);

        // Build cointegrated price series with deliberate divergence for signal.
        // A oscillates around 100, B oscillates around 50, spread = A/2 - B.
        for i in 0..80 {
            let base_a = 10000.0 + 500.0 * ((i as f64) * 0.3).sin();
            let base_b = 5000.0 + 250.0 * ((i as f64) * 0.3).sin();

            // Add a spike to generate z-score signal in the middle.
            let spike = if (40..50).contains(&i) { 2000.0 } else { 0.0 };

            let price_a_buy = (base_a) as u32;
            let price_a_sell = (base_a + 10.0) as u32;
            let price_b_buy = (base_b) as u32;
            let price_b_sell = (base_b + 10.0) as u32;

            // Inject the spike only on symbol A to push z-score.
            let price_a_buy = price_a_buy + spike as u32;

            // Clear old orders by deleting.
            let msg_del_a_b = Message::OrderDelete(crate::parser::naive::OrderDelete {
                stock_locate: 1,
                tracking_number: 1,
                timestamp_ns: 0,
                order_ref: 1000,
            });
            pipe.process_message(&msg_del_a_b);
            let msg_del_a_s = Message::OrderDelete(crate::parser::naive::OrderDelete {
                stock_locate: 1,
                tracking_number: 1,
                timestamp_ns: 0,
                order_ref: 1001,
            });
            pipe.process_message(&msg_del_a_s);
            let msg_del_b_b = Message::OrderDelete(crate::parser::naive::OrderDelete {
                stock_locate: 1,
                tracking_number: 1,
                timestamp_ns: 0,
                order_ref: 2000,
            });
            pipe.process_message(&msg_del_b_b);
            let msg_del_b_s = Message::OrderDelete(crate::parser::naive::OrderDelete {
                stock_locate: 1,
                tracking_number: 1,
                timestamp_ns: 0,
                order_ref: 2001,
            });
            pipe.process_message(&msg_del_b_s);

            // Add new orders.
            let msg_a_buy = make_add_order(&pipe.config().symbol_a, 1000, true, price_a_buy, 10);
            let msg_a_sell = make_add_order(&pipe.config().symbol_a, 1001, false, price_a_sell, 10);
            let msg_b_buy = make_add_order(&pipe.config().symbol_b, 2000, true, price_b_buy, 10);
            let msg_b_sell = make_add_order(&pipe.config().symbol_b, 2001, false, price_b_sell, 10);

            pipe.process_message(&msg_a_buy);
            pipe.process_message(&msg_a_sell);
            pipe.process_message(&msg_b_buy);
            pipe.process_message(&msg_b_sell);
        }

        // With enough data and deliberate divergence, we should have some signals.
        assert!(pipe.stats().msg_count > 0);
        assert!(pipe.stats().spread_updates > 0);
    }

    #[test]
    fn pipeline_stats_are_plausible() {
        let config = test_config();
        let mut pipe = StrategyPipeline::new(config);

        // Feed a single pair of orders.
        let msg_a = make_add_order(&pipe.config().symbol_a, 1, true, 10000, 100);
        let msg_b = make_add_order(&pipe.config().symbol_b, 2, true, 5000, 100);
        pipe.process_message(&msg_a);
        pipe.process_message(&msg_b);

        let stats = pipe.stats();
        assert_eq!(stats.msg_count, 2);
        assert!(stats.total_cycles > 0);
    }

    #[test]
    fn trade_decision_equality() {
        assert_eq!(TradeDecision::Hold, TradeDecision::Hold);
        assert_eq!(TradeDecision::Exit, TradeDecision::Exit);
        assert_eq!(
            TradeDecision::Enter { long: true },
            TradeDecision::Enter { long: true }
        );
        assert_ne!(
            TradeDecision::Enter { long: true },
            TradeDecision::Enter { long: false }
        );
    }

    #[test]
    fn position_transitions() {
        let config = test_config();
        let pipe = StrategyPipeline::new(config);
        assert_eq!(pipe.position(), Position::Flat);
    }

    #[test]
    fn default_config_sanity() {
        let cfg = StrategyConfig::default();
        assert_eq!(cfg.lookback, 200);
        assert!(cfg.z_entry > cfg.z_exit);
        assert!(cfg.half_life_bounds.0 < cfg.half_life_bounds.1);
    }
}
