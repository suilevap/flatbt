use core::fmt;

use super::GoalCall;
use crate::inspect::{Inspector, NodeInfo};
use crate::{BtNode, Entry, NodeResult};

/// A subgoal, run when there is one.
pub struct Need<F>(F);

/// Asks `subgoal(ctx, goal)` for what the current goal needs first. `None`
/// succeeds: nothing is in the way. `Some(g)` runs the goal subtree for `g`
/// one frame deeper and returns its result, so `seq((need(..), act))` acts
/// once the subgoal is achieved, and `select((need(a), need(b)))` tries `b`
/// when `a` cannot be achieved.
///
/// Fails without running `g` when it is already on the stack (a cycle), when
/// it already failed for this goal while the goal is on the stack, or when
/// the stack is full. The subgoal's frame ends when it completes, or when
/// an update no longer reaches this node, as a preempted branch does.
pub fn need<F>(subgoal: F) -> Need<F> {
    Need(subgoal)
}

impl<'p, C, A, G, F> BtNode<C, A, GoalCall<'p, C, A, G>> for Need<F>
where
    G: Clone + fmt::Debug + 'static,
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
        mut call: GoalCall<'p, C, A, G>,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        let Some(subgoal) = (self.0)(ctx, call.goal) else {
            return NodeResult::Success;
        };
        entry.record("goal", || subgoal.clone());
        call.need(ctx, subgoal, Entry::new(entry.mode()))
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("need", state.is_some()).with_fn_name::<F>();
        inspector.node(node, |_| {});
    }
}
