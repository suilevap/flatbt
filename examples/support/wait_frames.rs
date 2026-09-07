use flatbt::{BtNode, EntryMode, ExecutionCursor, NodeResult};

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
