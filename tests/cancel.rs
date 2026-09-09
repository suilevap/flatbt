use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flatbt::{BtAction, BtCancel, BtState, CancelOnDrop, EntryMode, NodeResult, action, update};

struct Request {
    cancellations: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

impl BtCancel for Request {
    fn cancel(&mut self) {
        self.cancellations.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

struct ExternalAction {
    cancellations: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
    success: bool,
}

impl BtAction<bool> for ExternalAction {
    type State = CancelOnDrop<Request>;

    fn start(&self, _: &mut bool, _: ()) -> Option<Self::State> {
        Some(CancelOnDrop::from(Request {
            cancellations: self.cancellations.clone(),
            drops: self.drops.clone(),
        }))
    }

    fn is_in_progress(&self, _: &Self::State, running: &bool, _: ()) -> bool {
        *running
    }

    fn complete(&self, state: &mut Self::State, _: &mut bool, _: ()) -> bool {
        state.disarm();
        self.success
    }
}

#[test]
fn cancellation_is_opt_in_and_completion_keeps_ordinary_destruction() {
    for (success, result) in [(true, NodeResult::Success), (false, NodeResult::Failure)] {
        let cancellations = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let root = action(ExternalAction {
            cancellations: cancellations.clone(),
            drops: drops.clone(),
            success,
        });
        let mut state = BtState::new(&root);
        assert_eq!(
            update(&root, &mut state, &mut false, EntryMode::Resume),
            result
        );
        assert_eq!(cancellations.load(Ordering::Relaxed), 0);
        assert_eq!(drops.load(Ordering::Relaxed), 1);

        assert_eq!(
            update(&root, &mut state, &mut true, EntryMode::Resume),
            NodeResult::Running
        );
        state.reset();
        state.reset();
        assert_eq!(cancellations.load(Ordering::Relaxed), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }
}
