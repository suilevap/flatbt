use flatbt::{BtNode, Entry, NodeResult};

/// Runs the nested node, then fails: a candidate its parent rejects.
/// Forwards its entire state to the nested node.
pub struct Reject<N>(pub N);

impl<C, N: BtNode<C>> BtNode<C> for Reject<N> {
    type State = N::State;
    type Memory = N::Memory;

    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut C,
        _: (),
        entry: Entry<'_>,
    ) -> NodeResult {
        let _ = self.0.update(state, memory, ctx, (), entry);
        NodeResult::Failure
    }
}
