use core::fmt::Debug;
use core::ops::Add;

use super::{BtOrder, Pass};
use crate::Entry;

/// Children ordered by score, highest first.
pub struct ByScore<ScoreFn, Score> {
    score: ScoreFn,
    inertia: Option<Score>,
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
pub fn by_score<ScoreFn, Score>(score: ScoreFn) -> ByScore<ScoreFn, Score> {
    ByScore {
        score,
        inertia: None,
    }
}

impl<ScoreFn, Score> ByScore<ScoreFn, Score> {
    /// Adds `bonus` to the running child's score, so a challenger has to beat
    /// it by more than `bonus` to go first.
    pub fn inertia(mut self, bonus: Score) -> Self {
        self.inertia = Some(bonus);
        self
    }
}

impl<Context, ScoreFn, Score> BtOrder<Context> for ByScore<ScoreFn, Score>
where
    ScoreFn: Fn(&Context, usize) -> Score,
    Score: Copy + PartialOrd + Add<Output = Score> + Debug + 'static,
{
    type State = ();
    type Memory = ();

    fn kind(&self) -> &'static str {
        "by_score"
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
        let mut best: Option<(usize, Score)> = None;
        for index in pass.left() {
            let mut score = (self.score)(ctx, index);
            // Every child is scored at the first position; record them there.
            if pass.is_start() {
                entry.record("score", || score);
            }
            if score.partial_cmp(&score).is_none() {
                continue;
            }
            let keeps = pass.running() == Some(index);
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
