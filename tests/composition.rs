use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flatbt::{BtNode, BtState, EntryMode, NodeResult, leaf, select, seq, update};

#[path = "../examples/support/wait_frames.rs"]
mod wait;
use wait::wait_frames;

struct Pending(Arc<AtomicUsize>);

#[derive(Default)]
struct PendingState(Option<Arc<AtomicUsize>>);

impl Drop for PendingState {
    fn drop(&mut self) {
        if let Some(drops) = &self.0 {
            drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl BtNode<usize> for Pending {
    type State = PendingState;

    fn update(&self, state: &mut PendingState, _: &mut usize, _: EntryMode) -> NodeResult {
        state.0.get_or_insert_with(|| self.0.clone());
        NodeResult::Running
    }
}

struct Reject<N>(N);

impl<C, N: BtNode<C>> BtNode<C> for Reject<N> {
    // This adapter forwards its entire state to the nested node.
    type State = N::State;

    fn update(&self, state: &mut Self::State, ctx: &mut C, mode: EntryMode) -> NodeResult {
        let _ = self.0.update(state, ctx, mode);
        NodeResult::Failure
    }
}

#[test]
fn rejected_static_candidate_drops_its_state_without_resetting_saved_branch() {
    let drops = Arc::new(AtomicUsize::new(0));
    let tree = select((
        Reject(seq((Pending(drops.clone()),))),
        seq((
            wait_frames(1),
            leaf(|calls: &mut usize| {
                *calls += 1;
                NodeResult::Success
            }),
        )),
    ));
    let mut state = BtState::new(&tree);
    let mut calls = 0;
    assert_eq!(
        update(&tree, &mut state, &mut calls, EntryMode::Resume),
        NodeResult::Running
    );
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(
        update(&tree, &mut state, &mut calls, EntryMode::Evaluate),
        NodeResult::Success
    );
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(calls, 1);
    assert!(!state.is_running());
}
