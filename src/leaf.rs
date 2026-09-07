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

/// Suspends for a fixed number of updates, then succeeds on the following update.
/// This counts updates, not elapsed wall-clock time, and requires no tick.
pub struct WaitFrames(usize);

/// `wait_frames(3)` returns Running three times, then Success.
/// `wait_frames(0)` succeeds immediately.
pub fn wait_frames(frames: usize) -> WaitFrames {
    WaitFrames(frames)
}

impl<C> BtNode<C> for WaitFrames {
    type State = usize;

    fn update(
        &self,
        elapsed: &mut usize,
        _: &mut C,
        _: &mut ExecutionCursor<'_>,
        _: EntryMode,
    ) -> NodeResult {
        if *elapsed < self.0 {
            *elapsed += 1;
            NodeResult::Running
        } else {
            NodeResult::Success
        }
    }
}
