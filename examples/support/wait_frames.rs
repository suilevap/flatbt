use flatbt::{BtNode, Entry, NodeResult};

/// Counts updates until completion. No wall-clock timing or tick required.
pub struct WaitFrames(usize);

/// `wait_frames(3)` returns Running three times, then Success.
/// `wait_frames(0)` succeeds immediately.
pub fn wait_frames(frames: usize) -> WaitFrames {
    WaitFrames(frames)
}

impl<C> BtNode<C> for WaitFrames {
    type State = usize;

    fn update(&self, elapsed: &mut usize, _: &mut C, _: (), _: Entry<'_>) -> NodeResult {
        if *elapsed < self.0 {
            *elapsed += 1;
            NodeResult::RUNNING
        } else {
            NodeResult::Success
        }
    }
}
