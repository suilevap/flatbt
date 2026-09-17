use crate::{BtNode, EntryMode, NodeResult};

/// Stateless callable. Captures hold configuration; context holds mutable data.
pub struct Leaf<F>(F);

/// Wraps a callable. Context effects are immediate and survive failure.
/// Implement [`BtNode`] directly for invocation-local state.
///
/// The callable is checked where the tree runs, not here, so a closure stays
/// open to inference. That is what lets one closure serve a context borrowed
/// for the update, whose lifetimes the tree only fixes when it is used.
pub fn leaf<F>(f: F) -> Leaf<F> {
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

/// Returns Success for true, Failure for false. Checked where the tree runs,
/// like [`leaf`].
pub fn check<F>(predicate: F) -> Check<F> {
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
