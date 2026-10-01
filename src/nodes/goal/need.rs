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
/// - `Some(g)`, new: pushes `g` and fails, ending this goal's turn. `g` runs
///   next, in the same update. When it ends, this goal's subtree runs again
///   from its start, and the same `need` returns how `g` ended.
/// - `Some(g)` that already ended for this goal: returns that result at once,
///   for as long as this goal is on the stack. A goal asks for each subgoal
///   at most once.
///
/// Fails without pushing when `g` is already on the stack (a cycle), when
/// another `need` pushed in this turn, or when the stack is full.
///
/// Put `need` last in its branch: in a `select`, nodes after a `need` that
/// pushed still run in that turn, before it ends.
pub fn need<F>(subgoal: F) -> Need<F> {
    Need(subgoal)
}

impl<'p, C, A, G, F> BtNode<C, A, GoalCall<'p, G>> for Need<F>
where
    G: Clone + PartialEq + fmt::Debug + 'static,
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
        match call.ask(subgoal) {
            Asked::Returned(true) => NodeResult::Success,
            Asked::Returned(false) => NodeResult::Failure,
            Asked::Pushed => {
                entry.record("pushed", || true);
                NodeResult::Failure
            }
            Asked::Cycle => {
                entry.record("cycle", || true);
                NodeResult::Failure
            }
            Asked::AlreadyPushed => NodeResult::Failure,
            Asked::Full => entry.error("goal stack is full"),
        }
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("need", state.is_some()).with_fn_name::<F>();
        inspector.node(node, |_| {});
    }
}
