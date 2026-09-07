use crate::{BtNode, NodeResult};

/// A synchronous leaf backed by an immutable callable.
pub struct Leaf<F>(F);

/// Creates a leaf. Mutable application data belongs in the context.
///
/// Context changes take effect immediately; there is no transaction or rollback.
pub fn leaf<C, F: Fn(&mut C) -> NodeResult>(f: F) -> Leaf<F> {
    Leaf(f)
}

impl<C, F: Fn(&mut C) -> NodeResult> BtNode<C> for Leaf<F> {
    fn update(&self, ctx: &mut C) -> NodeResult {
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
    fn update(&self, ctx: &mut C) -> NodeResult {
        if (self.0)(ctx) {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }
}
