use core::fmt;

use super::GoalCall;
use super::stack::Asked;
use crate::inspect::{Inspector, NodeInfo};
use crate::{BtNode, Entry, NodeResult};

/// A subgoal, asked for when there is one.
pub struct Need<F>(F);

/// Asks `subgoal(ctx, goal)` for what the current goal needs first.
///
/// - `None`: nothing is in the way; succeeds.
/// - `Some(g)`, new: pushes `g` and waits, `Running`. `g` runs next, in the
///   same update; when it ends, this goal resumes here and `need` returns how
///   `g` ended.
/// - `Some(g)` that already ended for this goal: returns that result at once,
///   for as long as this goal is on the stack. A goal asks for each subgoal
///   at most once.
///
/// Fails without pushing when `g` is already on the stack (a cycle) or the
/// stack is full.
///
/// While waiting it returns `Running(A::default())`, which [`goals`] replaces
/// with the subgoal's act; it never leaves the stack.
///
/// [`goals`]: super::goals
pub fn need<F>(subgoal: F) -> Need<F> {
    Need(subgoal)
}

impl<'p, C, A, G, F> BtNode<C, A, GoalCall<'p, G>> for Need<F>
where
    A: Default,
    G: Clone + PartialEq + fmt::Debug + Send + 'static,
    F: Fn(&C, &G) -> Option<G>,
{
    /// The subgoal this node waits for.
    type State = Option<G>;
    type Memory = ();

    #[inline]
    fn update(
        &self,
        waiting: &mut Option<G>,
        _: &mut (),
        ctx: &mut C,
        mut call: GoalCall<'p, G>,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        // Back from the subgoal it pushed.
        if let Some(subgoal) = waiting.take()
            && let Some(succeeded) = call.result(&subgoal)
        {
            entry.record("returned", || succeeded);
            return if succeeded {
                NodeResult::Success
            } else {
                NodeResult::Failure
            };
        }
        let Some(subgoal) = (self.0)(ctx, call.goal) else {
            return NodeResult::Success;
        };
        entry.record("goal", || subgoal.clone());
        match call.ask(subgoal.clone()) {
            Asked::Returned(true) => NodeResult::Success,
            Asked::Returned(false) => NodeResult::Failure,
            Asked::Pushed => {
                *waiting = Some(subgoal);
                NodeResult::Running(A::default())
            }
            Asked::Cycle => {
                entry.record("cycle", || true);
                NodeResult::Failure
            }
            Asked::Full => entry.error("goal stack is full"),
        }
    }

    fn inspect(&self, waiting: Option<&Option<G>>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("need", waiting.is_some()).with_fn_name::<F>();
        inspector.node(node, |inspector| {
            if let Some(Some(subgoal)) = waiting {
                inspector.field("waiting", subgoal);
            }
        });
    }
}
