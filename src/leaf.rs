use crate::{BtNode, EntryMode, ExecutionCursor, NodeResult};

/// A leaf backed by an immutable callable. Captured configuration stays in the
/// definition; mutable application data belongs in the context.
pub struct Leaf<F>(F);

/// Creates a stateless leaf. Context changes take effect immediately, without
/// rollback. For invocation-local state, implement `BtNode` directly.
pub fn leaf<C, F: Fn(&mut C) -> NodeResult>(f: F) -> Leaf<F> {
    Leaf(f)
}

impl<C, F: Fn(&mut C) -> NodeResult> BtNode<C> for Leaf<F> {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        ctx: &mut C,
        _: &mut ExecutionCursor<'_>,
        _: EntryMode,
    ) -> NodeResult {
        (self.0)(ctx)
    }
}

/// A condition with shared access to the context.
pub struct Check<F>(F);

/// Creates a condition that succeeds when its predicate is true.
pub fn check<C, F: Fn(&C) -> bool>(predicate: F) -> Check<F> {
    Check(predicate)
}

impl<C, F: Fn(&C) -> bool> BtNode<C> for Check<F> {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        ctx: &mut C,
        _: &mut ExecutionCursor<'_>,
        _: EntryMode,
    ) -> NodeResult {
        if (self.0)(ctx) {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }
}
