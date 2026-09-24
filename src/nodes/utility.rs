use core::ops::Add;

use crate::{BtControl, ControlNode, ControlOp, control};

/// Children tried in an invocation are one bit each in a `u64`.
const MAX_CHILDREN: usize = 64;

/// Kept cold and out of line, so the formatting stays off the path every
/// update takes.
#[cold]
#[inline(never)]
fn too_many_children(child_count: usize) -> ControlOp {
    ControlOp::error(format_args!(
        "utility supports at most {MAX_CHILDREN} children, got {child_count}"
    ))
}

/// Runs the best-scoring child; on failure, the best one not yet tried.
///
/// `score(ctx, index)` rates each child. Higher wins; a tie keeps the running
/// child, and otherwise goes to the lower index. A score that is not comparable with itself (NaN) skips the
/// child. Floats make a utility selector; integers make a dynamic priority
/// selector.
///
/// - Fresh entry and Evaluate score every child and run the best, so under
///   Evaluate a better child preempts the running one, and a worse one never
///   does. `inertia` is added to the running child's score, so a challenger
///   must beat it by more than that.
/// - Resume continues the running child without scoring.
/// - A child that fails is not tried again in the same invocation: the policy
///   rescores the rest and runs the best of them, or fails when none is left.
/// - A child that succeeds ends the node with Success.
///
/// At most 64 children; more reports a diagnostic and fails.
pub struct Utility<F, S> {
    score: F,
    inertia: Option<S>,
}

impl<F, S> Utility<F, S> {
    pub fn new(score: F) -> Self {
        Self {
            score,
            inertia: None,
        }
    }

    /// Adds `bonus` to the running child's score when the policy rescores, so
    /// a challenger has to beat it by more than `bonus` to take over.
    pub fn inertia(mut self, bonus: S) -> Self {
        self.inertia = Some(bonus);
        self
    }

    /// The untried child with the highest score, if any.
    #[inline]
    fn best<C>(&self, ctx: &C, tried: u64, count: usize, active: Option<usize>) -> Option<usize>
    where
        F: Fn(&C, usize) -> S,
        S: Copy + PartialOrd + Add<Output = S>,
    {
        let mut best: Option<(usize, S)> = None;
        for index in (0..count).filter(|index| tried & (1 << index) == 0) {
            let mut score = (self.score)(ctx, index);
            if score.partial_cmp(&score).is_none() {
                continue;
            }
            if let (Some(bonus), true) = (self.inertia, active == Some(index)) {
                score = score + bonus;
            }
            // Ties keep the running child, then go to the lower index.
            let keeps = active == Some(index);
            if best.is_none_or(|(_, best)| score > best || (keeps && score == best)) {
                best = Some((index, score));
            }
        }
        best.map(|(index, _)| index)
    }
}

/// Runs the best-scoring child, as [`Utility`] describes. [`crate::utility!`]
/// writes the scorer as one arm per child.
///
/// ```
/// use flatbt::prelude::*;
///
/// struct Needs { hunger: f32, fatigue: f32 }
///
/// let tree = utility(
///     |needs: &Needs, index: usize| if index == 0 { needs.hunger } else { needs.fatigue },
///     (
///         leaf(|_: &mut Needs| NodeResult::Running("eat")),
///         leaf(|_: &mut Needs| NodeResult::Running("sleep")),
///     ),
/// );
/// let mut state = BtState::new(&tree);
/// let mut needs = Needs { hunger: 0.2, fatigue: 0.9 };
/// assert_eq!(update(&tree, &mut state, &mut needs, EntryMode::Evaluate), NodeResult::Running("sleep"));
/// ```
///
/// For inertia, build the policy: `control(Utility::new(score).inertia(0.1), children)`.
pub fn utility<F, S, Children>(
    score: F,
    children: Children,
) -> ControlNode<Utility<F, S>, Children> {
    control(Utility::new(score), children)
}

impl<C, F, S> BtControl<C> for Utility<F, S>
where
    F: Fn(&C, usize) -> S,
    S: Copy + PartialOrd + Add<Output = S>,
{
    /// Children tried in this invocation, one bit each.
    type State = u64;

    #[inline(always)]
    fn begin(
        &self,
        tried: &mut u64,
        ctx: &mut C,
        active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp {
        if child_count > MAX_CHILDREN {
            return too_many_children(child_count);
        }
        *tried = 0;
        self.run_best(tried, ctx, child_count, active_child_index)
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
        _completed_child_index: usize,
        child_count: usize,
    ) -> ControlOp {
        self.run_best(tried, ctx, child_count, None)
    }
}

impl<F, S> Utility<F, S> {
    #[inline]
    fn run_best<C>(
        &self,
        tried: &mut u64,
        ctx: &C,
        count: usize,
        active: Option<usize>,
    ) -> ControlOp
    where
        F: Fn(&C, usize) -> S,
        S: Copy + PartialOrd + Add<Output = S>,
    {
        match self.best(ctx, *tried, count, active) {
            Some(index) => {
                *tried |= 1 << index;
                ControlOp::RunChild(index)
            }
            None => ControlOp::Failure,
        }
    }
}

/// Scores each child in its own arm and runs the best, as [`Utility`]
/// describes.
///
/// ```
/// use flatbt::prelude::*;
///
/// struct Needs { hunger: f32, fatigue: f32 }
///
/// let tree = utility!(|needs: &Needs| {
///     needs.hunger => leaf(|_: &mut Needs| NodeResult::Running("eat")),
///     needs.fatigue => leaf(|_: &mut Needs| NodeResult::Running("sleep")),
/// });
/// let mut state = BtState::new(&tree);
/// let mut needs = Needs { hunger: 0.8, fatigue: 0.3 };
/// assert_eq!(update(&tree, &mut state, &mut needs, EntryMode::Evaluate), NodeResult::Running("eat"));
/// ```
///
/// Each arm is `score => node`. A score is an expression over the context
/// argument, asked each time the policy chooses; node definitions are built
/// once, in arm order, without access to it. Add `, inertia = bonus` after the
/// braces to favour the running child. Use `move |ctx: &Context|` to own
/// captures. Limit: 64 arms, and `FLATBT_MAX_CHILDREN`.
#[macro_export]
macro_rules! utility {
    ([$($indices:literal)*] $($args:tt)*) => {
        $crate::utility!(@arms [$($indices)*] $($args)*)
    };
    (|$bb:ident: $context:ty| { $($arms:tt)* } $(, inertia = $inertia:expr)? $(,)?) => {
        $crate::utility!(@parse [[] [$bb: $context] [$($inertia)?]] [] ; $($arms)*)
    };
    (move |$bb:ident: $context:ty| { $($arms:tt)* } $(, inertia = $inertia:expr)? $(,)?) => {
        $crate::utility!(@parse [[move] [$bb: $context] [$($inertia)?]] [] ; $($arms)*)
    };
    (@parse $setup:tt [$($arms:tt)*] ; $score:expr => $node:expr $(, $($rest:tt)*)?) => {
        $crate::utility!(@parse $setup [$($arms)* [$score] [$node]] ; $($($rest)*)?)
    };
    (@parse $setup:tt [] ;) => {
        compile_error!("utility! needs at least one `score => node` arm")
    };
    (@parse [$($setup:tt)*] [$($arms:tt)*] ;) => {
        $crate::__flatbt_child_indices!([$crate::utility]; $($setup)* [] [] ; $($arms)*)
    };
    // The last arm takes `_`, so the scorer's match needs no unreachable arm.
    (@arms [$index:literal $($indices:literal)*]
        [$($capture:tt)*] [$bb:ident: $context:ty] [$($inertia:expr)?]
        [$($nodes:tt)*] [$($scores:tt)*] ;
        [$score:expr] [$node:expr]) => {
        $crate::control(
            $crate::Utility::new(
                $($capture)* |$bb: $context, index: usize| match index { $($scores)* _ => $score }
            ) $(.inertia($inertia))?,
            ($($nodes)* $node,),
        )
    };
    (@arms [$index:literal $($indices:literal)*]
        [$($capture:tt)*] [$bb:ident: $context:ty] [$($inertia:expr)?]
        [$($nodes:tt)*] [$($scores:tt)*] ;
        [$score:expr] [$node:expr] $($rest:tt)+) => {
        $crate::utility!(@arms [$($indices)*]
            [$($capture)*] [$bb: $context] [$($inertia)?]
            [$($nodes)* $node,]
            [$($scores)* $index => $score,]
            ; $($rest)+
        )
    };
    (@arms [] $($rest:tt)*) => {
        compile_error!("utility! arm count exceeds FLATBT_MAX_CHILDREN")
    };
}
