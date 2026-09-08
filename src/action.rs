use crate::{BtNode, EntryMode, NodeResult};

/// A convenience lifecycle executed directly inside BtNode::update.
/// All callbacks, including tick, may run while a branch is still speculative.
/// Context effects are not rolled back if a parent rejects that branch.
pub trait BtAction<C> {
    type State: Send + 'static;

    fn start(&self, ctx: &mut C) -> Option<Self::State>;
    fn is_in_progress(&self, state: &Self::State, ctx: &C) -> bool;
    fn tick(&self, state: &mut Self::State, ctx: &mut C);
    fn complete(&self, _state: &Self::State, _ctx: &mut C) -> bool {
        true
    }
}

/// An action adapted to the ordinary node protocol.
pub struct ActionNode<A>(A);

/// Creates a node with lazy action state. A::State does not need Default.
pub fn action<A>(action: A) -> ActionNode<A> {
    ActionNode(action)
}

impl<C, A: BtAction<C>> BtNode<C> for ActionNode<A> {
    type State = Option<A::State>;

    fn update(&self, state: &mut Self::State, ctx: &mut C, _: EntryMode) -> NodeResult {
        if state.is_none() {
            *state = self.0.start(ctx);
        }
        let Some(active) = state.as_mut() else {
            return NodeResult::Failure;
        };
        if self.0.is_in_progress(active, ctx) {
            self.0.tick(active, ctx);
            NodeResult::Running
        } else {
            let result = if self.0.complete(active, ctx) {
                NodeResult::Success
            } else {
                NodeResult::Failure
            };
            *state = None;
            result
        }
    }
}
