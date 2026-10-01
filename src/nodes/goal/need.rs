use core::fmt;

use super::GoalCall;
use crate::inspect::{Inspector, NodeInfo};
use crate::{BtNode, Entry, NodeResult};

/// A subgoal, asked for when there is one.
pub struct Need<F>(F);

/// Asks `subgoal(ctx, goal)` for what the current goal needs first.
///
/// - `None`: nothing is in the way; succeeds.
/// - `Some(g)` that already ended for this goal: returns how it ended, for as
///   long as this goal is on the stack. So a goal asks for each subgoal at most
///   once: on with the goal if `g` was achieved, on to another way if not.
/// - `Some(g)`, new: requests `g` from [`goals`] and stops this goal's run with
///   `Running`, so nothing after it runs. This goal runs again from its start
///   once `g` has ended, or has been refused as a cycle or for a full stack.
///
/// The `Running(A::default())` it stops with is a placeholder: [`goals`] runs
/// the subgoal in the same update and returns its act instead.
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
    type State = ();
    type Memory = ();

    #[inline]
    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut C,
        mut call: GoalCall<'p, G>,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        let Some(subgoal) = (self.0)(ctx, call.goal) else {
            return NodeResult::Success;
        };
        entry.record("goal", || subgoal.clone());
        match call.result(&subgoal) {
            Some(true) => NodeResult::Success,
            Some(false) => NodeResult::Failure,
            None => {
                call.request(subgoal);
                NodeResult::Running(A::default())
            }
        }
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("need", state.is_some()).with_fn_name::<F>();
        inspector.node(node, |_| {});
    }
}
