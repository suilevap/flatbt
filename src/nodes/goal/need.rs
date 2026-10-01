use core::fmt;

use super::GoalCall;
use crate::inspect::{Inspector, NodeInfo};
use crate::{BtNode, Entry, NodeResult};

/// A subgoal, asked for when there is one.
pub struct Need<Subgoal>(Subgoal);

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
/// The `Running(Act::default())` it stops with is a placeholder: [`goals`] runs
/// the subgoal in the same update and returns its act instead.
///
/// [`goals`]: super::goals
pub fn need<Subgoal>(subgoal: Subgoal) -> Need<Subgoal> {
    Need(subgoal)
}

impl<'p, Context, Act, Goal, Subgoal> BtNode<Context, Act, GoalCall<'p, Goal>> for Need<Subgoal>
where
    Act: Default,
    Goal: Clone + PartialEq + fmt::Debug + Send + 'static,
    Subgoal: Fn(&Context, &Goal) -> Option<Goal>,
{
    type State = ();
    type Memory = ();

    #[inline]
    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        mut call: GoalCall<'p, Goal>,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let Some(subgoal) = (self.0)(ctx, call.goal) else {
            return NodeResult::Success;
        };
        entry.record("goal", || subgoal.clone());
        match call.result(&subgoal) {
            Some(true) => NodeResult::Success,
            Some(false) => NodeResult::Failure,
            None => {
                call.request(subgoal);
                NodeResult::Running(Act::default())
            }
        }
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("need", state.is_some()).with_fn_name::<Subgoal>();
        inspector.node(node, |_| {});
    }
}
