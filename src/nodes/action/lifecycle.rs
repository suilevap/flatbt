use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult};

/// Inline lifecycle: start, query progress, tick while Running, then complete.
/// All callbacks may run on rejected candidates; effects are not rolled back.
///
/// An action is what occupies an agent, so it is also what says what the agent
/// is doing: [`tick`](Self::tick) returns the act, and the node reports it as
/// [`NodeResult::Running`]. It is asked on every update the action is still in
/// progress, which is how a long action follows a moving target -- restating
/// where it is going without ending.
///
/// State owns cancellation. Use [`crate::CancelOnDrop`] or a custom Drop;
/// disarm in complete when cancellation is no longer needed. No cancel traversal.
/// `Params` carries inputs/outputs separately from `Context`. Parameters are borrowed each
/// update; store owned snapshots in State when needed.
pub trait BtAction<Context, Act = (), Params = ()> {
    type State: Send + 'static;

    fn start(&self, ctx: &mut Context, params: Params) -> Option<Self::State>;
    fn is_in_progress(&self, state: &Self::State, ctx: &Context, params: Params) -> bool;

    /// Runs inline while progress is true, and says what the agent is doing.
    ///
    /// Required rather than defaulted: an action is what occupies the agent, so
    /// being busy without saying with what is the state this design removes. In
    /// a tree that decides nothing the act type is `()` and the body is empty;
    /// external operations that advance on their own also do no work here, they
    /// just restate the act.
    fn tick(&self, state: &mut Self::State, ctx: &mut Context, params: Params) -> Act;

    /// Handles completion before state drops. Disarm cancellation handles here.
    fn complete(&self, _state: &mut Self::State, _ctx: &mut Context, _params: Params) -> bool {
        true
    }

    /// Reports fields for debug views, such as a target or progress: from
    /// configuration, and from `state` once the action has started. Reports
    /// nothing by default. See [`crate::inspect`].
    fn inspect(&self, _state: Option<&Self::State>, _inspector: &mut dyn Inspector) {}
}

/// Adapts an action to [`BtNode`].
pub struct ActionNode<Action>(Action);

/// Initializes action state on entry. `Action::State` need not implement Default.
pub fn action<Action>(action: Action) -> ActionNode<Action> {
    ActionNode(action)
}

impl<Context, Act, Params: ParamValue, Action, ActionState> BtNode<Context, Act, Params>
    for ActionNode<Action>
where
    Action: for<'a> BtAction<
            Context,
            Act,
            <Params::Shape as ParamShape>::Value<'a>,
            State = ActionState,
        >,
    ActionState: Send + 'static,
{
    type State = Option<ActionState>;
    type Memory = ();

    fn update(
        &self,
        state: &mut Self::State,
        _: &mut (),
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let mut params = params.into_value();
        if state.is_none() {
            *state = self.0.start(ctx, Params::Shape::reborrow(&mut params));
            let started = state.is_some();
            entry.record("started", || started);
        }
        let Some(active) = state.as_mut() else {
            return NodeResult::Failure;
        };
        if self
            .0
            .is_in_progress(active, ctx, Params::Shape::reborrow(&mut params))
        {
            NodeResult::Running(
                self.0
                    .tick(active, ctx, Params::Shape::reborrow(&mut params)),
            )
        } else {
            let completed = self
                .0
                .complete(active, ctx, Params::Shape::reborrow(&mut params));
            entry.record("completed", || completed);
            let result = if completed {
                NodeResult::Success
            } else {
                NodeResult::Failure
            };
            *state = None;
            result
        }
    }

    fn inspect(&self, state: Option<&Option<ActionState>>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("action", state.is_some()).with_type_name::<Action>();
        inspector.node(node, |inspector| {
            let started = state.and_then(Option::as_ref);
            BtAction::<Context, Act, <Params::Shape as ParamShape>::Value<'_>>::inspect(
                &self.0, started, inspector,
            );
        });
    }
}
