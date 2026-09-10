use flatbt_core::params::{ParamShape, ParamValue};
use flatbt_core::{BtNode, EntryMode, NodeResult};

/// Inline lifecycle: start, query progress, tick while Running, then complete.
/// All callbacks may run on rejected candidates; effects are not rolled back.
///
/// State owns cancellation. Use [`crate::CancelOnDrop`] or a custom Drop;
/// disarm in complete when cancellation is no longer needed. No cancel traversal.
/// `P` carries inputs/outputs separately from `C`. Parameters are borrowed each
/// update; store owned snapshots in State when needed.
pub trait BtAction<C, P = ()> {
    type State: Send + 'static;

    fn start(&self, ctx: &mut C, params: P) -> Option<Self::State>;
    fn is_in_progress(&self, state: &Self::State, ctx: &C, params: P) -> bool;

    /// Runs inline while progress is true. Defaults to no work.
    /// External operations can advance independently of BT updates.
    fn tick(&self, _state: &mut Self::State, _ctx: &mut C, _params: P) {}

    /// Handles completion before state drops. Disarm cancellation handles here.
    fn complete(&self, _state: &mut Self::State, _ctx: &mut C, _params: P) -> bool {
        true
    }
}

/// Adapts an action to [`BtNode`].
pub struct ActionNode<A>(A);

/// Initializes action state on entry. `A::State` need not implement Default.
pub fn action<A>(action: A) -> ActionNode<A> {
    ActionNode(action)
}

impl<C, P: ParamValue, A, S> BtNode<C, P> for ActionNode<A>
where
    A: for<'a> BtAction<C, <P::Shape as ParamShape>::Value<'a>, State = S>,
    S: Send + 'static,
{
    type State = Option<S>;

    fn update(&self, state: &mut Self::State, ctx: &mut C, params: P, _: EntryMode) -> NodeResult {
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
            self.0.tick(active, ctx, P::Shape::reborrow(&mut params));
            NodeResult::Running
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
