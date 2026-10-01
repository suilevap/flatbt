//! Random orders. All randomness comes from the context, `rng: Fn(&mut C) ->
//! u32`, one draw per position: the game owns the generator, and tests stay
//! deterministic.
//!
//! A random order keeps the running child first when a pass restarts under
//! `Evaluate`, so a random choice holds while it runs rather than being drawn
//! again every update. The rest of the pass is drawn afresh.

use super::{BtOrder, Pass};
use crate::Entry;

/// Children in a uniformly random order.
pub struct Shuffled<Rng>(Rng);

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
pub fn shuffled<Rng>(rng: Rng) -> Shuffled<Rng> {
    Shuffled(rng)
}

impl<Context, Rng: Fn(&mut Context) -> u32> BtOrder<Context> for Shuffled<Rng> {
    type State = ();
    type Memory = ();

    fn kind(&self) -> &'static str {
        "shuffled"
    }

    #[inline]
    fn next(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        pass: Pass,
        _: Entry<'_>,
    ) -> Option<usize> {
        if pass.is_start() && pass.running().is_some() {
            return pass.running();
        }
        let left = pass.left_count();
        if left == 0 {
            return None;
        }
        // Modulo bias is below 64 / 2^32: negligible for choosing behavior.
        let nth = (self.0)(ctx) as usize % left;
        pass.left().nth(nth)
    }
}

/// Children in a random order, drawn by weight.
pub struct Weighted<Rng, Weight> {
    rng: Rng,
    weight: Weight,
}

/// Orders children at random, drawing each position with probability
/// proportional to `weight(ctx, index)` among the children left. A weight
/// that is not positive, NaN included, leaves its child out.
///
/// One draw from `rng` per position, like [`shuffled`]; the weights are read
/// as each position is drawn.
pub fn weighted<Rng, Weight>(rng: Rng, weight: Weight) -> Weighted<Rng, Weight> {
    Weighted { rng, weight }
}

impl<Context, Rng, Weight> BtOrder<Context> for Weighted<Rng, Weight>
where
    Rng: Fn(&mut Context) -> u32,
    Weight: Fn(&Context, usize) -> f32,
{
    type State = ();
    type Memory = ();

    fn kind(&self) -> &'static str {
        "weighted"
    }

    #[inline]
    fn next(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        pass: Pass,
        entry: Entry<'_>,
    ) -> Option<usize> {
        if pass.is_start() && pass.running().is_some() {
            return pass.running();
        }
        let weight = |ctx: &Context, index: usize| {
            let weight = (self.weight)(ctx, index);
            // `> 0.0` is false for NaN too.
            if weight > 0.0 { weight } else { 0.0 }
        };
        let total: f32 = pass
            .left()
            .map(|index| {
                let weight = weight(ctx, index);
                // Every child is weighed at the first position; record them there.
                if pass.is_start() {
                    entry.record("weight", || weight);
                }
                weight
            })
            .sum();
        if total <= 0.0 || !total.is_finite() {
            return None;
        }
        let draw = (self.rng)(ctx) as f32 / (u32::MAX as f32 + 1.0) * total;
        let mut last = None;
        let mut sum = 0.0;
        for index in pass.left() {
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
