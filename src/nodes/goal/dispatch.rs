use super::GoalCall;
use crate::inspect::Inspector;
use crate::{BtNode, Entry, NodeResult};

/// A subtree for the goals a predicate accepts.
pub struct WhenGoal<Accepts, Node> {
    accepts: Accepts,
    child: Node,
}

/// Runs `child` when the current goal is one `accepts`, and fails otherwise.
/// [`crate::goal_match!`] writes one per arm under a `select`.
pub fn when_goal<Accepts, Node>(accepts: Accepts, child: Node) -> WhenGoal<Accepts, Node> {
    WhenGoal { accepts, child }
}

impl<'p, Context, Act, Goal, Accepts, Node, NodeState, NodeMemory>
    BtNode<Context, Act, GoalCall<'p, Goal>> for WhenGoal<Accepts, Node>
where
    Context: 'static,
    Act: 'static,
    Goal: 'static,
    Accepts: Fn(&Goal) -> bool,
    Node: for<'a> BtNode<Context, Act, GoalCall<'a, Goal>, State = NodeState, Memory = NodeMemory>,
    NodeState: Default + Send + 'static,
    NodeMemory: Default + Send + 'static,
{
    type State = NodeState;
    type Memory = NodeMemory;
    const NODES: usize = <Node as BtNode<Context, Act, GoalCall<'static, Goal>>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut Context,
        call: GoalCall<'p, Goal>,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
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
        BtNode::<Context, Act, GoalCall<'_, Goal>>::inspect(&self.child, state, memory, inspector);
    }
}

/// A node that reads the current goal.
pub struct WithGoal<Node>(Node);

/// Runs `node` with the current goal as its parameter, `&Goal`: for an action
/// or leaf that needs to know what it works toward.
pub fn with_goal<Node>(node: Node) -> WithGoal<Node> {
    WithGoal(node)
}

impl<'p, Context, Act, Goal, Node, NodeState, NodeMemory> BtNode<Context, Act, GoalCall<'p, Goal>>
    for WithGoal<Node>
where
    Goal: 'static,
    Node: for<'a> BtNode<Context, Act, &'a Goal, State = NodeState, Memory = NodeMemory>,
    NodeState: Default + Send + 'static,
    NodeMemory: Default + Send + 'static,
{
    type State = NodeState;
    type Memory = NodeMemory;
    const NODES: usize = <Node as BtNode<Context, Act, &'static Goal>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut NodeState,
        memory: &mut NodeMemory,
        ctx: &mut Context,
        call: GoalCall<'p, Goal>,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        self.0.update(state, memory, ctx, call.goal, entry)
    }

    fn inspect(
        &self,
        state: Option<&NodeState>,
        memory: &NodeMemory,
        inspector: &mut dyn Inspector,
    ) {
        BtNode::<Context, Act, &Goal>::inspect(&self.0, state, memory, inspector);
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
