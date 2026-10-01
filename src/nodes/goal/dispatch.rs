use super::GoalCall;
use crate::inspect::Inspector;
use crate::{BtNode, Entry, NodeResult};

/// A subtree for the goals a predicate accepts.
pub struct WhenGoal<F, N> {
    accepts: F,
    child: N,
}

/// Runs `child` when the current goal is one `accepts`, and fails otherwise.
/// [`crate::goal_match!`] writes one per arm under a `select`.
pub fn when_goal<F, N>(accepts: F, child: N) -> WhenGoal<F, N> {
    WhenGoal { accepts, child }
}

impl<'p, C, A, G, F, N, S, M> BtNode<C, A, GoalCall<'p, G>> for WhenGoal<F, N>
where
    C: 'static,
    A: 'static,
    G: 'static,
    F: Fn(&G) -> bool,
    N: for<'a> BtNode<C, A, GoalCall<'a, G>, State = S, Memory = M>,
    S: Default + Send + 'static,
    M: Default + Send + 'static,
{
    type State = S;
    type Memory = M;
    const NODES: usize = <N as BtNode<C, A, GoalCall<'static, G>>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut C,
        call: GoalCall<'p, G>,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        if (self.accepts)(call.goal) {
            self.child.update(state, memory, ctx, call, entry)
        } else {
            NodeResult::Failure
        }
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &Self::Memory,
        inspector: &mut dyn Inspector,
    ) {
        BtNode::<C, A, GoalCall<'_, G>>::inspect(&self.child, state, memory, inspector);
    }
}

/// A node that reads the current goal.
pub struct WithGoal<N>(N);

/// Runs `node` with the current goal as its parameter, `&G`: for an action
/// or leaf that needs to know what it works toward.
pub fn with_goal<N>(node: N) -> WithGoal<N> {
    WithGoal(node)
}

impl<'p, C, A, G, N, S, M> BtNode<C, A, GoalCall<'p, G>> for WithGoal<N>
where
    G: 'static,
    N: for<'a> BtNode<C, A, &'a G, State = S, Memory = M>,
    S: Default + Send + 'static,
    M: Default + Send + 'static,
{
    type State = S;
    type Memory = M;
    const NODES: usize = <N as BtNode<C, A, &'static G>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut S,
        memory: &mut M,
        ctx: &mut C,
        call: GoalCall<'p, G>,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        self.0.update(state, memory, ctx, call.goal, entry)
    }

    fn inspect(&self, state: Option<&S>, memory: &M, inspector: &mut dyn Inspector) {
        BtNode::<C, A, &G>::inspect(&self.0, state, memory, inspector);
    }
}

/// Dispatches on the current goal inside [`goals`](super::goals): one subtree
/// per pattern, tried in order.
///
/// ```ignore
/// goal_match!(|goal: &Goal| {
///     Goal::MoveTo(_) => select((direct_move, need(blocker))),
///     Goal::Unblock(_) => ..,
/// })
/// ```
///
/// Each arm is `when_goal(|goal| matches!(goal, pattern), subtree)` under a
/// `select`, so a goal no pattern accepts fails. Bindings in a pattern are not
/// visible to the subtree; closures read them from the goal.
#[macro_export]
macro_rules! goal_match {
    (|$goal:ident: &$ty:ty| { $($pattern:pat $(if $guard:expr)? => $node:expr),+ $(,)? }) => {
        $crate::select(($(
            $crate::inspect::label(
                stringify!($pattern $(if $guard)?),
                $crate::nodes::goal::when_goal(
                    |$goal: &$ty| matches!($goal, $pattern $(if $guard)?),
                    $node,
                ),
            ),
        )+))
    };
}
