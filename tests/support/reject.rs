use flatbt::{BtNode, EntryMode, NodeResult};

/// Runs the nested node, then fails: a candidate its parent rejects.
/// Forwards its entire state to the nested node.
pub struct Reject<N>(pub N);

impl<C, N: BtNode<C>> BtNode<C> for Reject<N> {
    type State = N::State;

    fn update(&self, state: &mut Self::State, ctx: &mut C, _: (), mode: EntryMode) -> NodeResult {
        let _ = self.0.update(state, ctx, (), mode);
        NodeResult::Failure
    }
}
