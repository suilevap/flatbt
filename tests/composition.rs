use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flatbt::prelude::*;

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

    fn update(&self, state: &mut PendingState, _: &mut usize, _: (), _: EntryMode) -> NodeResult {
        state.0.get_or_insert_with(|| self.0.clone());
        NodeResult::Running
    }
}

struct Reject<N>(N);

impl<C, N: BtNode<C>> BtNode<C> for Reject<N> {
    // This adapter forwards its entire state to the nested node.
    type State = N::State;

    fn update(&self, state: &mut Self::State, ctx: &mut C, _: (), mode: EntryMode) -> NodeResult {
        let _ = self.0.update(state, ctx, (), mode);
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

#[test]
fn adding_alternatives_does_not_multiply_persistent_state_size() {
    struct Wide;
    impl BtNode<()> for Wide {
        type State = [u64; 32];

        fn update(&self, _: &mut Self::State, _: &mut (), _: (), _: EntryMode) -> NodeResult {
            NodeResult::Running
        }
    }
    fn state_size<N: BtNode<()>>(_: &N) -> usize {
        std::mem::size_of::<N::State>()
    }

    let one = seq((Wide,));
    let eight = seq((Wide, Wide, Wide, Wide, Wide, Wide, Wide, Wide));
    // Leave room for discriminant/alignment differences without fixing Rust's ABI.
    assert!(state_size(&eight) < 2 * state_size(&one));
    println!(
        "eight children: control_state={} bt_state={}",
        state_size(&eight),
        std::mem::size_of_val(&BtState::new(&eight)),
    );
}

#[cfg(all(feature = "choose", feature = "scope"))]
#[test]
fn scoped_parameters_reach_the_chosen_node_without_the_action_adapter() {
    struct Observe;
    impl BtNode<Vec<u32>, &u32> for Observe {
        type State = ();

        fn update(&self, _: &mut (), ctx: &mut Vec<u32>, input: &u32, _: EntryMode) -> NodeResult {
            ctx.push(*input);
            NodeResult::Success
        }
    }

    let tree = scope! {
        context: Vec<u32>;
        let value: u32 = |_| 42;
        sequence {
            choose!(|trace: &Vec<u32>| match trace.is_empty() {
                true => Observe,
                false => leaf(|_: &mut Vec<u32>| NodeResult::Failure),
            }).with(value);
        }
    };
    let mut state = BtState::new(&tree);
    let mut trace = Vec::new();
    assert_eq!(
        update(&tree, &mut state, &mut trace, EntryMode::Resume),
        NodeResult::Success
    );
    assert_eq!(trace, [42]);
    assert_eq!(
        update(&tree, &mut state, &mut trace, EntryMode::Resume),
        NodeResult::Failure
    );
    assert_eq!(trace, [42]);
}
