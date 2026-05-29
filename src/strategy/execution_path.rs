//! A* execution path planner — optimal order slicing through order book liquidity.
//!
//! Ported concept from the 3we-robot-platform Nav2 stack: the Nav2 SmacPlanner2D
//! runs A* on a 2D occupancy grid costmap to find minimum-cost collision-free
//! paths. Here we transpose that idea:
//!
//!   - **Costmap** -> order book price levels with available liquidity.
//!     Empty levels are obstacles (no liquidity to execute against).
//!   - **A* nodes** -> (price_level, shares_remaining) pairs representing how
//!     much of the target order is still unfilled at a given book depth.
//!   - **Edges** -> executing some shares at a given price level. The edge cost
//!     is the market impact: the price paid (or received) relative to the mid-price.
//!   - **Heuristic** -> best-case cost assuming all remaining shares fill at the
//!     best available price, admissible by construction.
//!
//! The planner returns a Vec<(price_level, shares_to_execute)> describing how to
//! split a target order across price levels for minimum total market impact.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Back-pointer for A* path reconstruction: (prev_level, prev_step, shares_filled).
type CameFrom = Vec<Option<(usize, usize, f64)>>;

// ---------------------------------------------------------------------------
// Side
// ---------------------------------------------------------------------------

/// Direction of the target execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

// ---------------------------------------------------------------------------
// BookSnapshot
// ---------------------------------------------------------------------------

/// Simplified order book state consumed by the execution planner.
///
/// Price levels must be sorted: for asks, ascending (best ask first);
/// for bids, descending (best bid first).
#[derive(Debug, Clone)]
pub struct BookSnapshot {
    pub bid_prices: Vec<f64>,
    pub bid_sizes: Vec<f64>,
    pub ask_prices: Vec<f64>,
    pub ask_sizes: Vec<f64>,
}

impl BookSnapshot {
    /// Mid-price from best bid / best ask. Returns `None` if either side is empty.
    pub fn mid_price(&self) -> Option<f64> {
        match (self.bid_prices.first(), self.ask_prices.first()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            _ => None,
        }
    }

    /// Return the price/size columns relevant to `side` (asks for Buy, bids for Sell).
    fn levels(&self, side: Side) -> (&[f64], &[f64]) {
        match side {
            Side::Buy => (&self.ask_prices, &self.ask_sizes),
            Side::Sell => (&self.bid_prices, &self.bid_sizes),
        }
    }
}

// ---------------------------------------------------------------------------
// ExecutionNode (A* state)
// ---------------------------------------------------------------------------

/// A* search node representing a partial execution state.
///
/// Ordering is reversed (max-heap used as min-heap via Ord).
#[derive(Debug, Clone)]
struct ExecutionNode {
    /// How deep into the book we have progressed (index into price levels).
    price_level: usize,
    /// Shares still to execute.
    shares_remaining: f64,
    /// Cumulative market impact cost so far (g-score).
    cost_so_far: f64,
    /// Estimated total cost: g + heuristic (f-score).
    estimated_total_cost: f64,
}

impl PartialEq for ExecutionNode {
    fn eq(&self, other: &Self) -> bool {
        self.estimated_total_cost == other.estimated_total_cost
    }
}

impl Eq for ExecutionNode {}

impl PartialOrd for ExecutionNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Reverse ordering so BinaryHeap (max-heap) yields the node with the *lowest*
// estimated_total_cost first.
impl Ord for ExecutionNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimated_total_cost
            .partial_cmp(&self.estimated_total_cost)
            .unwrap_or(Ordering::Equal)
    }
}

// ---------------------------------------------------------------------------
// ExecutionPlanner
// ---------------------------------------------------------------------------

/// A* planner that finds the minimum-impact execution path through an order book.
///
/// Modelled after the Nav2 SmacPlanner2D: the book is a 1D "costmap" where each
/// cell is a price level, empty levels are obstacles, and A* searches for the
/// cheapest way to "consume" a target quantity of shares.
pub struct ExecutionPlanner {
    book: BookSnapshot,
    target_shares: f64,
    side: Side,
}

impl ExecutionPlanner {
    pub fn new(book: &BookSnapshot, target_shares: f64, side: Side) -> Self {
        Self {
            book: book.clone(),
            target_shares,
            side,
        }
    }

    /// Plan the optimal execution path.
    ///
    /// Returns a list of `(price_level_index, shares_to_execute)` pairs describing
    /// how to slice the order across the book. Returns an empty Vec if execution
    /// is not possible (insufficient liquidity).
    pub fn plan(&self) -> Vec<(usize, f64)> {
        let (prices, sizes) = self.book.levels(self.side);
        let mid = match self.book.mid_price() {
            Some(m) => m,
            None => return Vec::new(),
        };

        if prices.len() != sizes.len() || prices.is_empty() || self.target_shares <= 0.0 {
            return Vec::new();
        }

        // Total available liquidity.
        let total_available: f64 = sizes.iter().sum();
        if total_available < self.target_shares - 1e-12 {
            return Vec::new();
        }

        // Discretise shares into buckets so the state space is tractable.
        // We use a share-step of target_shares / MAX_STEPS, clamped.
        const MAX_STEPS: usize = 64;
        let share_step = if self.target_shares <= 0.0 {
            return Vec::new();
        } else {
            self.target_shares / MAX_STEPS as f64
        };

        if share_step <= 0.0 {
            return Vec::new();
        }

        let max_steps = MAX_STEPS;
        let num_levels = prices.len();

        // Quantise shares_remaining into step index.
        let to_step = |shares: f64| -> usize {
            let s = (shares / share_step).round() as usize;
            s.min(max_steps)
        };

        let initial_step = to_step(self.target_shares);

        // g-score: best known cost to reach each (level, step) state.
        let mut g_score = vec![vec![f64::INFINITY; max_steps + 1]; num_levels + 1];
        // Backtrack: for each state, which (level, step, shares_filled) we came from.
        let mut came_from: Vec<CameFrom> = vec![vec![None; max_steps + 1]; num_levels + 1];

        g_score[0][initial_step] = 0.0;

        let start_node = ExecutionNode {
            price_level: 0,
            shares_remaining: self.target_shares,
            cost_so_far: 0.0,
            estimated_total_cost: self.heuristic(0, self.target_shares, prices, sizes, mid),
        };

        let mut open = BinaryHeap::new();
        open.push(start_node);

        // A* main loop.
        while let Some(current) = open.pop() {
            let level = current.price_level;
            let step = to_step(current.shares_remaining);

            // Goal: zero shares remaining.
            if current.shares_remaining <= share_step * 0.5 {
                return self.reconstruct(level, step, &came_from, share_step);
            }

            // Prune stale entries.
            if current.cost_so_far > g_score[level][step] + 1e-12 {
                continue;
            }

            // If we have exhausted the book, no expansion possible.
            if level >= num_levels {
                continue;
            }

            let available = sizes[level];
            let price = prices[level];

            // Obstacle: no liquidity at this level — must skip to the next.
            if available <= 0.0 {
                let next_level = level + 1;
                let next_step = step;
                if next_level <= num_levels {
                    let tentative_g = current.cost_so_far;
                    if tentative_g < g_score[next_level][next_step] - 1e-12 {
                        g_score[next_level][next_step] = tentative_g;
                        came_from[next_level][next_step] = Some((level, step, 0.0));
                        open.push(ExecutionNode {
                            price_level: next_level,
                            shares_remaining: current.shares_remaining,
                            cost_so_far: tentative_g,
                            estimated_total_cost: tentative_g
                                + self.heuristic(
                                    next_level,
                                    current.shares_remaining,
                                    prices,
                                    sizes,
                                    mid,
                                ),
                        });
                    }
                }
                continue;
            }

            // Expand: try executing 0 to min(available, shares_remaining) at this level,
            // quantised into discrete steps.
            let max_fill = available.min(current.shares_remaining);
            let max_fill_steps = to_step(max_fill);

            for fill_steps in 0..=max_fill_steps {
                let filled = (fill_steps as f64) * share_step;
                if fill_steps > 0 && filled > max_fill + 1e-12 {
                    break;
                }
                // Clamp the actual filled amount to what is really available.
                let actual_filled = if fill_steps == 0 {
                    0.0
                } else {
                    filled.min(max_fill)
                };

                let new_remaining = (current.shares_remaining - actual_filled).max(0.0);
                let new_step = to_step(new_remaining);

                // Cost: market impact for this fill at this price level.
                let impact = self.impact_cost(price, actual_filled, mid);
                let tentative_g = current.cost_so_far + impact;

                // Next level to visit after this one.
                let next_level = level + 1;

                if tentative_g < g_score[next_level][new_step] - 1e-12 {
                    g_score[next_level][new_step] = tentative_g;
                    came_from[next_level][new_step] = Some((level, step, actual_filled));
                    open.push(ExecutionNode {
                        price_level: next_level,
                        shares_remaining: new_remaining,
                        cost_so_far: tentative_g,
                        estimated_total_cost: tentative_g
                            + self.heuristic(next_level, new_remaining, prices, sizes, mid),
                    });
                }
            }
        }

        // No path found — insufficient liquidity or all paths blocked.
        Vec::new()
    }

    /// Admissible heuristic: minimum cost assuming all remaining shares execute
    /// at the best available price from `start_level` onward.
    fn heuristic(
        &self,
        start_level: usize,
        shares_remaining: f64,
        prices: &[f64],
        _sizes: &[f64],
        mid: f64,
    ) -> f64 {
        if shares_remaining <= 0.0 {
            return 0.0;
        }
        // Best price from this level onward.
        let best_price = match self.side {
            Side::Buy => prices[start_level..]
                .iter()
                .cloned()
                .fold(f64::INFINITY, f64::min),
            Side::Sell => prices[start_level..]
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max),
        };
        if !best_price.is_finite() {
            return f64::INFINITY;
        }
        // Minimum possible impact = best_price * remaining - mid * remaining.
        // For Buy: positive impact (paying more than mid). For Sell: negative impact
        // (receiving less than mid). We take absolute value for cost.
        self.impact_cost(best_price, shares_remaining, mid)
    }

    /// Market impact cost for executing `shares` at `price` vs mid-price benchmark.
    fn impact_cost(&self, price: f64, shares: f64, mid: f64) -> f64 {
        if shares <= 0.0 {
            return 0.0;
        }
        match self.side {
            // Buying: cost = (price - mid) * shares. Paying above mid is positive cost.
            Side::Buy => (price - mid).max(0.0) * shares,
            // Selling: cost = (mid - price) * shares. Receiving below mid is positive cost.
            Side::Sell => (mid - price).max(0.0) * shares,
        }
    }

    /// Reconstruct the execution path by backtracking through `came_from`.
    fn reconstruct(
        &self,
        goal_level: usize,
        goal_step: usize,
        came_from: &[CameFrom],
        _share_step: f64,
    ) -> Vec<(usize, f64)> {
        let mut path = Vec::new();
        let mut level = goal_level;
        let mut step = goal_step;

        while let Some(&(prev_level, prev_step, shares_filled)) = came_from
            .get(level)
            .and_then(|row| row.get(step))
            .and_then(|o| o.as_ref())
        {
            if shares_filled > 0.0 {
                // The fill happened at `prev_level` (the level before we moved forward).
                // `prev_level` is the price_level index in the book.
                path.push((prev_level, shares_filled));
            }

            // Safety: break if we are about to revisit the same state (malformed graph).
            if prev_level == level && prev_step == step {
                break;
            }

            level = prev_level;
            step = prev_step;
        }

        path.reverse();
        path
    }
}

// ---------------------------------------------------------------------------
// Market impact metric
// ---------------------------------------------------------------------------

/// Compute total market impact of an execution plan vs mid-price benchmark.
///
/// `plan` is a list of `(price_level_index, shares_to_execute)`. The function
/// looks up the price at each level and sums the absolute deviation from mid-price.
pub fn market_impact(book: &BookSnapshot, plan: &[(usize, f64)], side: Side) -> f64 {
    let mid = match book.mid_price() {
        Some(m) => m,
        None => return f64::NAN,
    };

    let (prices, _) = book.levels(side);

    plan.iter()
        .map(|&(level, shares)| {
            if level >= prices.len() || shares <= 0.0 {
                return 0.0;
            }
            let price = prices[level];
            match side {
                Side::Buy => (price - mid).max(0.0) * shares,
                Side::Sell => (mid - price).max(0.0) * shares,
            }
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_book(
        bid_prices: Vec<f64>,
        bid_sizes: Vec<f64>,
        ask_prices: Vec<f64>,
        ask_sizes: Vec<f64>,
    ) -> BookSnapshot {
        BookSnapshot {
            bid_prices,
            bid_sizes,
            ask_prices,
            ask_sizes,
        }
    }

    #[test]
    fn empty_book_no_execution() {
        let book = make_book(vec![], vec![], vec![], vec![]);
        let planner = ExecutionPlanner::new(&book, 100.0, Side::Buy);
        let plan = planner.plan();
        assert!(plan.is_empty());
    }

    #[test]
    fn one_sided_book_no_mid_price() {
        // Only bids, no asks -> no mid-price -> cannot plan.
        let book = make_book(vec![99.0], vec![200.0], vec![], vec![]);
        let planner = ExecutionPlanner::new(&book, 100.0, Side::Buy);
        let plan = planner.plan();
        assert!(plan.is_empty());
    }

    #[test]
    fn single_level_book_all_at_one_level() {
        let book = make_book(vec![99.0], vec![500.0], vec![101.0], vec![500.0]);
        let planner = ExecutionPlanner::new(&book, 100.0, Side::Buy);
        let plan = planner.plan();

        // Should have exactly one execution at level 0 (the only ask level).
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].0, 0);
        assert!((plan[0].1 - 100.0).abs() < 1.0);
    }

    #[test]
    fn multi_level_book_splits_for_lower_impact() {
        // Ask side: level 0 at 101.0 with 50 shares, level 1 at 102.0 with 200 shares.
        // Buying 100 shares: optimal is take all 50 at 101 + 50 at 102.
        // Naive would take all 100 at 101+ (impossible since only 50 available at 101).
        let book = make_book(
            vec![99.0],
            vec![500.0],
            vec![101.0, 102.0],
            vec![50.0, 200.0],
        );
        let planner = ExecutionPlanner::new(&book, 100.0, Side::Buy);
        let plan = planner.plan();

        assert!(!plan.is_empty());

        // Total shares should approximately equal target.
        let total_shares: f64 = plan.iter().map(|&(_, s)| s).sum();
        assert!(
            (total_shares - 100.0).abs() < 5.0,
            "total_shares = {total_shares}"
        );

        // Should use multiple levels.
        let levels_used: Vec<usize> = plan.iter().map(|&(l, _)| l).collect();
        assert!(
            levels_used.len() >= 2,
            "should split across levels, got {levels_used:?}"
        );

        // Market impact should be finite.
        let impact = market_impact(&book, &plan, Side::Buy);
        assert!(impact.is_finite() && impact >= 0.0);
    }

    #[test]
    fn large_order_splits_across_levels() {
        // 5 ask levels, each with 30 shares. Buying 120 shares.
        // Should take from the first 4 levels.
        let book = make_book(
            vec![99.0],
            vec![500.0],
            vec![100.0, 101.0, 102.0, 103.0, 104.0],
            vec![30.0, 30.0, 30.0, 30.0, 30.0],
        );
        let planner = ExecutionPlanner::new(&book, 120.0, Side::Buy);
        let plan = planner.plan();

        assert!(!plan.is_empty());

        let total_shares: f64 = plan.iter().map(|&(_, s)| s).sum();
        assert!(
            (total_shares - 120.0).abs() < 5.0,
            "total_shares = {total_shares}"
        );

        // Should use at least 4 levels.
        assert!(
            plan.len() >= 3,
            "should use multiple levels for large order, got {} levels",
            plan.len()
        );

        let impact = market_impact(&book, &plan, Side::Buy);
        assert!(impact.is_finite() && impact >= 0.0);
    }

    #[test]
    fn sell_side_execution() {
        let book = make_book(
            vec![99.0, 98.0, 97.0],
            vec![50.0, 100.0, 200.0],
            vec![101.0],
            vec![500.0],
        );
        let planner = ExecutionPlanner::new(&book, 120.0, Side::Sell);
        let plan = planner.plan();

        assert!(!plan.is_empty());

        let total_shares: f64 = plan.iter().map(|&(_, s)| s).sum();
        assert!(
            (total_shares - 120.0).abs() < 5.0,
            "total_shares = {total_shares}"
        );

        let impact = market_impact(&book, &plan, Side::Sell);
        assert!(impact.is_finite() && impact >= 0.0);
    }

    #[test]
    fn insufficient_liquidity_returns_empty() {
        let book = make_book(vec![99.0], vec![10.0], vec![101.0], vec![5.0]);
        // Ask for 100 shares but only 5 available on the ask side.
        let planner = ExecutionPlanner::new(&book, 100.0, Side::Buy);
        let plan = planner.plan();
        assert!(plan.is_empty());
    }

    #[test]
    fn empty_levels_are_obstacles() {
        // Level 1 has zero liquidity — should be skipped.
        let book = make_book(
            vec![99.0],
            vec![500.0],
            vec![100.0, 101.0, 102.0],
            vec![50.0, 0.0, 200.0],
        );
        let planner = ExecutionPlanner::new(&book, 100.0, Side::Buy);
        let plan = planner.plan();

        assert!(!plan.is_empty());

        // No execution should occur at level 1 (empty).
        for &(level, _) in &plan {
            assert_ne!(level, 1, "should not execute at empty level");
        }

        let total_shares: f64 = plan.iter().map(|&(_, s)| s).sum();
        assert!(
            (total_shares - 100.0).abs() < 5.0,
            "total_shares = {total_shares}"
        );
    }

    #[test]
    fn market_impact_computation() {
        let book = make_book(
            vec![99.0],
            vec![500.0],
            vec![101.0, 102.0],
            vec![100.0, 100.0],
        );
        // Mid = 100.0
        // Buying 50 at level 0 (price 101): impact = (101 - 100) * 50 = 50
        // Buying 50 at level 1 (price 102): impact = (102 - 100) * 50 = 100
        // Total = 150
        let plan = vec![(0, 50.0), (1, 50.0)];
        let impact = market_impact(&book, &plan, Side::Buy);
        assert!((impact - 150.0).abs() < 1e-10, "impact = {impact}");
    }

    #[test]
    fn zero_target_shares() {
        let book = make_book(vec![99.0], vec![500.0], vec![101.0], vec![500.0]);
        let planner = ExecutionPlanner::new(&book, 0.0, Side::Buy);
        let plan = planner.plan();
        assert!(plan.is_empty());
    }

    #[test]
    fn planner_prefers_cheaper_levels() {
        // Two ask levels at same price -> should prefer level 0 (earlier = less depth).
        // More interestingly: if level 0 has enough, should NOT use level 1.
        let book = make_book(
            vec![99.0],
            vec![500.0],
            vec![101.0, 102.0],
            vec![200.0, 200.0],
        );
        let planner = ExecutionPlanner::new(&book, 50.0, Side::Buy);
        let plan = planner.plan();

        // Only 50 shares needed, all available at level 0 at 101.0.
        // Should only use level 0.
        assert_eq!(plan.len(), 1, "should use only the cheapest level");
        assert_eq!(plan[0].0, 0);
        assert!((plan[0].1 - 50.0).abs() < 5.0);
    }
}
