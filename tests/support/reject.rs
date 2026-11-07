use flatbt::{BtNode, Entry, NodeResult};

/// Runs the nested node, then fails: a candidate its parent rejects.
/// Forwards its entire state to the nested node.
pub struct Reject<Node>(pub Node);

impl<Blackboard, Act, Node: BtNode<Blackboard, Act>> BtNode<Blackboard, Act> for Reject<Node> {
    type State = Node::State;
    type Memory = Node::Memory;

    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        bb: &mut Blackboard,
        _: (),
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let _ = self.0.update(state, memory, bb, (), entry);
        NodeResult::Failure
    }
}
