use core::ops::Add;

use super::BtOrder;

/// Children ordered by score, highest first.
pub struct ByScore<F, S> {
    score: F,
    inertia: Option<S>,
}

/// Orders children by `score(ctx, index)`, highest first. Under
/// `select(order_by(..))` this is a utility selector: the best child runs, a
/// better one preempts it under `Evaluate`, and when it fails the best of the
/// rest runs.
///
/// A tie keeps the running child, and otherwise goes to the lower index. A
/// score that is not comparable with itself (NaN) leaves its child out.
/// Integer scores make a dynamic priority order. [`crate::utility!`] writes
/// the scorer as one arm per child.
pub fn by_score<F, S>(score: F) -> ByScore<F, S> {
    ByScore {
        score,
        inertia: None,
    }
}

impl<F, S> ByScore<F, S> {
    /// Adds `bonus` to the running child's score, so a challenger has to beat
    /// it by more than `bonus` to go first.
    pub fn inertia(mut self, bonus: S) -> Self {
        self.inertia = Some(bonus);
        self
    }
}

impl<C, F, S> BtOrder<C> for ByScore<F, S>
where
    F: Fn(&C, usize) -> S,
    S: Copy + PartialOrd + Add<Output = S>,
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
        let mut best: Option<(usize, S)> = None;
        for index in (0..child_count).filter(|index| used & (1 << index) == 0) {
            let mut score = (self.score)(ctx, index);
            if score.partial_cmp(&score).is_none() {
                continue;
            }
            let keeps = running == Some(index);
            if let (Some(bonus), true) = (self.inertia, keeps) {
                score = score + bonus;
            }
            // Ties keep the running child, then go to the lower index.
            if best.is_none_or(|(_, best)| score > best || (keeps && score == best)) {
                best = Some((index, score));
            }
        }
        best.map(|(index, _)| index)
    }
}

/// A utility selector: scores each child in its own arm and runs the best,
/// falling back to the next best when it fails.
///
/// Shorthand for `select(order_by(by_score(..), children))`; see [`by_score`].
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
/// argument, asked each time the order is computed; node definitions are
/// built once, in arm order, without access to it. Add `, inertia = bonus`
/// after the braces to favour the running child. Use `move |ctx: &Context|` to
/// own captures. Limit: 64 arms, and `FLATBT_MAX_CHILDREN`.
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
        $crate::select($crate::nodes::order_by(
            $crate::nodes::by_score(
                $($capture)* |$bb: $context, index: usize| match index { $($scores)* _ => $score }
            ) $(.inertia($inertia))?,
            ($($nodes)* $node,),
        ))
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
