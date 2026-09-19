use flatbt_core::params::{ParamShape, ParamValue};
use flatbt_core::{BtNode, EntryMode, NodeResult};

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
/// `P` carries inputs/outputs separately from `C`. Parameters are borrowed each
/// update; store owned snapshots in State when needed.
pub trait BtAction<C, A = (), P = ()> {
    type State: Send + 'static;

    fn start(&self, ctx: &mut C, params: P) -> Option<Self::State>;
    fn is_in_progress(&self, state: &Self::State, ctx: &C, params: P) -> bool;

    /// Runs inline while progress is true, and says what the agent is doing.
    ///
    /// Required rather than defaulted: an action is what occupies the agent, so
    /// being busy without saying with what is the state this design removes. In
    /// a tree that decides nothing the act type is `()` and the body is empty;
    /// external operations that advance on their own also do no work here, they
    /// just restate the act.
    fn tick(&self, state: &mut Self::State, ctx: &mut C, params: P) -> A;

    /// Handles completion before state drops. Disarm cancellation handles here.
    fn complete(&self, _state: &mut Self::State, _ctx: &mut C, _params: P) -> bool {
        true
    }
}

/// Adapts an action to [`BtNode`].
pub struct ActionNode<T>(T);

/// Initializes action state on entry. `T::State` need not implement Default.
pub fn action<T>(action: T) -> ActionNode<T> {
    ActionNode(action)
}

impl<C, A, P: ParamValue, T, S> BtNode<C, A, P> for ActionNode<T>
where
    T: for<'a> BtAction<C, A, <P::Shape as ParamShape>::Value<'a>, State = S>,
    S: Send + 'static,
{
    type State = Option<S>;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: P,
        _: EntryMode,
    ) -> NodeResult<A> {
        let mut params = params.into_value();
        if state.is_none() {
            *state = self.0.start(ctx, P::Shape::reborrow(&mut params));
        }
        let Some(active) = state.as_mut() else {
            return NodeResult::Failure;
        };
        if self
            .0
            .is_in_progress(active, ctx, P::Shape::reborrow(&mut params))
        {
            NodeResult::Running(self.0.tick(active, ctx, P::Shape::reborrow(&mut params)))
        } else {
            let result = if self
                .0
                .complete(active, ctx, P::Shape::reborrow(&mut params))
            {
                NodeResult::Success
            } else {
                NodeResult::Failure
            };
            *state = None;
            result
        }
    }
}
