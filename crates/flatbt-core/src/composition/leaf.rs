use crate::{BtNode, EntryMode, NodeResult};

/// Stateless callable. Captures hold configuration; context holds mutable data.
pub struct Leaf<F>(F);

/// Wraps a callable. Context effects are immediate and survive failure.
/// Implement [`BtNode`] directly for invocation-local state.
pub fn leaf<C, F: Fn(&mut C) -> NodeResult>(f: F) -> Leaf<F> {
    Leaf(f)
}

impl<C, P, F: Fn(&mut C) -> NodeResult> BtNode<C, P> for Leaf<F> {
    type State = ();

    fn update(&self, _: &mut (), ctx: &mut C, _: P, _: EntryMode) -> NodeResult {
        (self.0)(ctx)
    }
}

/// Predicate over shared context.
pub struct Check<F>(F);

/// Returns Success for true, Failure for false.
pub fn check<C, F: Fn(&C) -> bool>(predicate: F) -> Check<F> {
    Check(predicate)
}

impl<C, P, F: Fn(&C) -> bool> BtNode<C, P> for Check<F> {
    type State = ();

    fn update(&self, _: &mut (), ctx: &mut C, _: P, _: EntryMode) -> NodeResult {
        if (self.0)(ctx) {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }
}
