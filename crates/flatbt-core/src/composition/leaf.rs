use crate::{BtNode, EntryMode, NodeResult};

/// Stateless callable. Captures hold configuration; context holds mutable data.
pub struct Leaf<F>(F);

/// Wraps a callable. Context effects are immediate and survive failure.
/// Implement [`BtNode`] directly for invocation-local state.
///
/// The callable is checked where the tree runs, not here, so a closure stays
/// open to inference. That is what lets one closure serve a context borrowed
/// for the update, whose lifetimes the tree only fixes when it is used.
///
/// A leaf that returns `Running` has to say what the agent is doing, which is
/// usually a sign it wants writing as an action instead -- a leaf is re-entered
/// on every resume and has no state to make progress with.
pub fn leaf<F>(f: F) -> Leaf<F> {
    Leaf(f)
}

impl<C, A, P, F: Fn(&mut C) -> NodeResult<A>> BtNode<C, A, P> for Leaf<F> {
    type State = ();

    fn update(&self, _: &mut (), ctx: &mut C, _: P, _: EntryMode) -> NodeResult<A> {
        (self.0)(ctx)
    }
}

/// Predicate over shared context.
pub struct Check<F>(F);

/// Returns Success for true, Failure for false. Checked where the tree runs,
/// like [`leaf`].
///
/// A predicate never occupies the agent, so it never names the act type.
pub fn check<F>(predicate: F) -> Check<F> {
    Check(predicate)
}

impl<C, A, P, F: Fn(&C) -> bool> BtNode<C, A, P> for Check<F> {
    type State = ();

    fn update(&self, _: &mut (), ctx: &mut C, _: P, _: EntryMode) -> NodeResult<A> {
        if (self.0)(ctx) {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }
}
