//! Random orders. Randomness comes from the context, `rng: Fn(&mut C) -> u32`,
//! so a game keeps one seeded generator and tests stay deterministic. One draw
//! seeds the invocation; each position's pick is derived from that seed, so a
//! pass that restarts under `Evaluate` walks the same order again, and no
//! permutation needs storing.

use super::BtOrder;

/// A 32-bit hash of the seed and a position (SplitMix64's finaliser).
#[inline(always)]
fn mix(seed: u32, position: u32) -> u32 {
    let mut z = ((seed as u64) << 32 | position as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 32) as u32
}

/// The seed for this invocation, drawn once.
#[inline(always)]
fn seed<C>(seed: &mut Option<u32>, rng: &impl Fn(&mut C) -> u32, ctx: &mut C) -> u32 {
    *seed.get_or_insert_with(|| rng(ctx))
}

/// Children in a uniformly random order.
pub struct Shuffled<R>(R);

/// Orders children uniformly at random, drawn once per invocation.
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
    type State = Option<u32>;

    #[inline]
    fn next(
        &self,
        state: &mut Option<u32>,
        ctx: &mut C,
        used: u64,
        _: Option<usize>,
        child_count: usize,
    ) -> Option<usize> {
        let left = child_count - used.count_ones() as usize;
        if left == 0 {
            return None;
        }
        let draw = mix(seed(state, &self.0, ctx), used.count_ones());
        // Modulo bias is below 64 / 2^32: negligible for choosing behavior.
        (0..child_count)
            .filter(|index| used & (1 << index) == 0)
            .nth(draw as usize % left)
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
/// Drawn once per invocation, like [`shuffled`]; the weights are read as each
/// position is drawn.
pub fn weighted<R, W>(rng: R, weight: W) -> Weighted<R, W> {
    Weighted { rng, weight }
}

impl<C, R, W> BtOrder<C> for Weighted<R, W>
where
    R: Fn(&mut C) -> u32,
    W: Fn(&C, usize) -> f32,
{
    type State = Option<u32>;

    #[inline]
    fn next(
        &self,
        state: &mut Option<u32>,
        ctx: &mut C,
        used: u64,
        _: Option<usize>,
        child_count: usize,
    ) -> Option<usize> {
        let seed = seed(state, &self.rng, ctx);
        let weight = |index: usize| {
            let weight = (self.weight)(ctx, index);
            // `> 0.0` is false for NaN too.
            if weight > 0.0 { weight } else { 0.0 }
        };
        let candidates = || (0..child_count).filter(|index| used & (1 << index) == 0);
        let total: f32 = candidates().map(weight).sum();
        if total <= 0.0 || !total.is_finite() {
            return None;
        }
        let draw = mix(seed, used.count_ones()) as f32 / (u32::MAX as f32 + 1.0) * total;
        let mut last = None;
        let mut sum = 0.0;
        for index in candidates() {
            let weight = weight(index);
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
