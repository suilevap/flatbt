//! Random orders. All randomness comes from the context, `rng: Fn(&mut C) ->
//! u32`, one draw per position: the game owns the generator, and tests stay
//! deterministic.
//!
//! A random order keeps the running child first when a pass restarts under
//! `Evaluate`, so a random choice holds while it runs rather than being drawn
//! again every update. The rest of the pass is drawn afresh.

use super::BtOrder;

/// Children in a uniformly random order.
pub struct Shuffled<R>(R);

/// Orders children uniformly at random, one draw from `rng` per position.
///
/// `select(order_by(shuffled(rng), ..))` runs a random child and falls back to
/// a random one of the rest; `seq(order_by(shuffled(rng), ..))` runs them all
/// in random order.
///
/// ```
/// use flatbt::prelude::*;
///
/// // A counter stands in for a generator.
/// let tree = seq(order_by(
///     shuffled(|n: &mut u32| { *n += 1; *n }),
///     (
///         leaf(|_: &mut u32| NodeResult::Success),
///         leaf(|_: &mut u32| NodeResult::Success),
///     ),
/// ));
/// let mut state: BtState<_, _> = BtState::new(&tree);
/// assert_eq!(update(&tree, &mut state, &mut 0, EntryMode::Evaluate), NodeResult::Success);
/// ```
pub fn shuffled<R>(rng: R) -> Shuffled<R> {
    Shuffled(rng)
}

impl<C, R: Fn(&mut C) -> u32> BtOrder<C> for Shuffled<R> {
    type State = ();

    #[inline]
    fn next(
        &self,
        _: &mut (),
        ctx: &mut C,
        used: u64,
        running: Option<usize>,
        child_count: usize,
    ) -> Option<usize> {
        if used == 0 && running.is_some() {
            return running;
        }
        let left = child_count - used.count_ones() as usize;
        if left == 0 {
            return None;
        }
        // Modulo bias is below 64 / 2^32: negligible for choosing behavior.
        let nth = (self.0)(ctx) as usize % left;
        (0..child_count)
            .filter(|index| used & (1 << index) == 0)
            .nth(nth)
    }
}

/// Children in a random order, drawn by weight.
pub struct Weighted<R, W> {
    rng: R,
    weight: W,
}

/// Orders children at random, drawing each position with probability
/// proportional to `weight(ctx, index)` among the children left. A weight
/// that is not positive, NaN included, leaves its child out.
///
/// One draw from `rng` per position, like [`shuffled`]; the weights are read
/// as each position is drawn.
pub fn weighted<R, W>(rng: R, weight: W) -> Weighted<R, W> {
    Weighted { rng, weight }
}

impl<C, R, W> BtOrder<C> for Weighted<R, W>
where
    R: Fn(&mut C) -> u32,
    W: Fn(&C, usize) -> f32,
{
    type State = ();

    #[inline]
    fn next(
        &self,
        _: &mut (),
        ctx: &mut C,
        used: u64,
        running: Option<usize>,
        child_count: usize,
    ) -> Option<usize> {
        if used == 0 && running.is_some() {
            return running;
        }
        let weight = |ctx: &C, index: usize| {
            let weight = (self.weight)(ctx, index);
            // `> 0.0` is false for NaN too.
            if weight > 0.0 { weight } else { 0.0 }
        };
        let candidates = || (0..child_count).filter(|index| used & (1 << index) == 0);
        let total: f32 = candidates().map(|index| weight(ctx, index)).sum();
        if total <= 0.0 || !total.is_finite() {
            return None;
        }
        let draw = (self.rng)(ctx) as f32 / (u32::MAX as f32 + 1.0) * total;
        let mut last = None;
        let mut sum = 0.0;
        for index in candidates() {
            let weight = weight(ctx, index);
            if weight == 0.0 {
                continue;
            }
            sum += weight;
            last = Some(index);
            if draw < sum {
                break;
            }
        }
        // Rounding can leave `draw` at `total`; the last candidate takes it.
        last
    }
}
