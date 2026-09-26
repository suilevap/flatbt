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
/// Integer scores make a dynamic priority order. [`crate::per_child!`] writes
/// the scorer as one arm per child, next to the child it scores.
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
