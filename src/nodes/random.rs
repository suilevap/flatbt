//! Random selection. Randomness comes from the context, `rng: Fn(&mut C) ->
//! u32`, so a game keeps one seeded generator and tests stay deterministic.
//! Children tried in an invocation are one bit each in a `u64`: at most 64
//! children; more reports a diagnostic and fails.

use crate::{BtControl, ControlNode, ControlOp, control};

const MAX_CHILDREN: usize = 64;

#[inline(always)]
fn too_many(child_count: usize) -> Option<ControlOp> {
    (child_count > MAX_CHILDREN).then(|| {
        ControlOp::error(format_args!(
            "random selection supports at most {MAX_CHILDREN} children, got {child_count}"
        ))
    })
}

#[inline(always)]
fn untried(tried: u64, index: usize) -> bool {
    tried & (1 << index) == 0
}

/// Marks and runs the `nth` untried child; `nth` must be below their count.
#[inline(always)]
fn run_nth_untried(tried: &mut u64, child_count: usize, nth: usize) -> ControlOp {
    match (0..child_count)
        .filter(|index| untried(*tried, *index))
        .nth(nth)
    {
        Some(index) => {
            *tried |= 1 << index;
            ControlOp::RunChild(index)
        }
        None => ControlOp::Failure,
    }
}

/// Marks and runs an untried child drawn uniformly, or `none` when all were tried.
#[inline(always)]
fn run_uniform<C>(
    rng: &impl Fn(&mut C) -> u32,
    tried: &mut u64,
    ctx: &mut C,
    child_count: usize,
    none: ControlOp,
) -> ControlOp {
    let left = child_count - tried.count_ones() as usize;
    if left == 0 {
        return none;
    }
    // Modulo bias is below 64 / 2^32: negligible for choosing behavior.
    let nth = rng(ctx) as usize % left;
    run_nth_untried(tried, child_count, nth)
}

/// Picks a child at random and falls back to another one when it fails.
pub struct RandomSelect<R>(R);

/// Runs a child drawn at random; when it fails, one drawn from the rest.
///
/// - Fresh entry draws uniformly among all children.
/// - Evaluate and Resume keep the running child: a random choice is not
///   reconsidered while it runs.
/// - A failed child is not tried again in the invocation. Fails when none is
///   left; succeeds with the first child that succeeds.
///
/// ```
/// use flatbt::prelude::*;
///
/// // A counter stands in for a generator.
/// let tree = random_select(
///     |n: &mut u32| { *n += 1; *n },
///     (
///         leaf(|_: &mut u32| NodeResult::Running("idle")),
///         leaf(|_: &mut u32| NodeResult::Running("wander")),
///     ),
/// );
/// let mut state = BtState::new(&tree);
/// let mut n = 0;
/// assert_eq!(update(&tree, &mut state, &mut n, EntryMode::Evaluate).act(), Some("wander"));
/// ```
pub fn random_select<R, Children>(
    rng: R,
    children: Children,
) -> ControlNode<RandomSelect<R>, Children> {
    control(RandomSelect(rng), children)
}

impl<C, R: Fn(&mut C) -> u32> BtControl<C> for RandomSelect<R> {
    type State = u64;

    #[inline(always)]
    fn begin(
        &self,
        tried: &mut u64,
        ctx: &mut C,
        active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp {
        if let Some(error) = too_many(child_count) {
            return error;
        }
        if let Some(index) = active_child_index {
            return ControlOp::RunChild(index);
        }
        *tried = 0;
        run_uniform(&self.0, tried, ctx, child_count, ControlOp::Failure)
    }

    #[inline(always)]
    fn child_succeeded(&self, _: &mut u64, _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Success
    }

    #[inline(always)]
    fn child_failed(
        &self,
        tried: &mut u64,
        ctx: &mut C,
        _: usize,
        child_count: usize,
    ) -> ControlOp {
        run_uniform(&self.0, tried, ctx, child_count, ControlOp::Failure)
    }
}

/// Picks a child at random by weight and falls back to another one when it fails.
pub struct WeightedSelect<R, W> {
    rng: R,
    weight: W,
}

/// [`random_select`] with `weight(ctx, index)` per child: a child is drawn
/// with probability proportional to its weight among those not yet tried.
/// A weight that is not positive, NaN included, never draws its child.
///
/// ```
/// use flatbt::prelude::*;
///
/// let tree = weighted_select(
///     |n: &mut u32| { *n = n.wrapping_add(1); *n },
///     |_: &u32, index: usize| if index == 0 { 0.0 } else { 1.0 },
///     (
///         leaf(|_: &mut u32| NodeResult::Running("never")),
///         leaf(|_: &mut u32| NodeResult::Running("always")),
///     ),
/// );
/// let mut state = BtState::new(&tree);
/// let mut n = 0;
/// assert_eq!(update(&tree, &mut state, &mut n, EntryMode::Evaluate).act(), Some("always"));
/// ```
pub fn weighted_select<R, W, Children>(
    rng: R,
    weight: W,
    children: Children,
) -> ControlNode<WeightedSelect<R, W>, Children> {
    control(WeightedSelect { rng, weight }, children)
}

impl<R, W> WeightedSelect<R, W> {
    #[inline(always)]
    fn run_weighted<C>(&self, tried: &mut u64, ctx: &mut C, child_count: usize) -> ControlOp
    where
        R: Fn(&mut C) -> u32,
        W: Fn(&C, usize) -> f32,
    {
        let weight = |ctx: &C, index: usize| {
            let weight = (self.weight)(ctx, index);
            // `> 0.0` is false for NaN too.
            if weight > 0.0 { weight } else { 0.0 }
        };
        let candidates = || (0..child_count).filter(|index| untried(*tried, *index));
        let total: f32 = candidates().map(|index| weight(ctx, index)).sum();
        if total <= 0.0 || !total.is_finite() {
            return ControlOp::Failure;
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
        match last {
            Some(index) => {
                *tried |= 1 << index;
                ControlOp::RunChild(index)
            }
            None => ControlOp::Failure,
        }
    }
}

impl<C, R: Fn(&mut C) -> u32, W: Fn(&C, usize) -> f32> BtControl<C> for WeightedSelect<R, W> {
    type State = u64;

    #[inline(always)]
    fn begin(
        &self,
        tried: &mut u64,
        ctx: &mut C,
        active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp {
        if let Some(error) = too_many(child_count) {
            return error;
        }
        if let Some(index) = active_child_index {
            return ControlOp::RunChild(index);
        }
        *tried = 0;
        self.run_weighted(tried, ctx, child_count)
    }

    #[inline(always)]
    fn child_succeeded(&self, _: &mut u64, _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Success
    }

    #[inline(always)]
    fn child_failed(
        &self,
        tried: &mut u64,
        ctx: &mut C,
        _: usize,
        child_count: usize,
    ) -> ControlOp {
        self.run_weighted(tried, ctx, child_count)
    }
}

/// A sequence in random order.
pub struct ShuffleSeq<R>(R);

/// Runs every child once, in an order drawn at random; fails on the first
/// failure, like [`seq`](crate::seq).
///
/// The next child is drawn when the previous one succeeds, among those not yet
/// run. Evaluate and Resume keep the running child. Empty succeeds.
pub fn shuffle_seq<R, Children>(
    rng: R,
    children: Children,
) -> ControlNode<ShuffleSeq<R>, Children> {
    control(ShuffleSeq(rng), children)
}

impl<C, R: Fn(&mut C) -> u32> BtControl<C> for ShuffleSeq<R> {
    type State = u64;

    #[inline(always)]
    fn begin(
        &self,
        tried: &mut u64,
        ctx: &mut C,
        active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp {
        if let Some(error) = too_many(child_count) {
            return error;
        }
        if let Some(index) = active_child_index {
            return ControlOp::RunChild(index);
        }
        *tried = 0;
        run_uniform(&self.0, tried, ctx, child_count, ControlOp::Success)
    }

    #[inline(always)]
    fn child_succeeded(
        &self,
        tried: &mut u64,
        ctx: &mut C,
        _: usize,
        child_count: usize,
    ) -> ControlOp {
        run_uniform(&self.0, tried, ctx, child_count, ControlOp::Success)
    }

    #[inline(always)]
    fn child_failed(&self, _: &mut u64, _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Failure
    }
}
