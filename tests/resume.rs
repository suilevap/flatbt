use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flatbt::{
    BtNode, BtState, EntryMode, ExecutionCursor, NodeResult, control, leaf, select, seq,
    wait_frames,
};

#[path = "../examples/support/mod.rs"]
mod support;
use NodeResult::{Failure, Running, Success};
use support::Repeat;

#[test]
fn sequence_resumes_wait_and_fires_in_the_completion_update() {
    let tree = seq((
        leaf(|trace: &mut Vec<&str>| {
            trace.push("check");
            Success
        }),
        wait_frames(3),
        leaf(|trace: &mut Vec<&str>| {
            trace.push("fire");
            Success
        }),
    ));
    let mut state = BtState::new(&tree);
    let mut trace = vec![];
    for _ in 0..3 {
        assert_eq!(state.update(&mut trace), Running);
        assert_eq!(trace, ["check"]);
        assert!(state.is_running());
    }
    assert_eq!(state.update(&mut trace), Success);
    assert_eq!(trace, ["check", "fire"]);
    assert!(!state.is_running());

    assert_eq!(state.update(&mut trace), Running);
    assert_eq!(trace, ["check", "fire", "check"]);
}

#[test]
fn selector_resumes_selected_branch_without_rescanning_priority() {
    #[derive(Default)]
    struct Context {
        urgent: bool,
        scans: usize,
    }
    let tree = select((
        leaf(|ctx: &mut Context| {
            ctx.scans += 1;
            if ctx.urgent { Success } else { Failure }
        }),
        wait_frames(1),
    ));
    let mut state = BtState::new(&tree);
    let mut ctx = Context::default();
    assert_eq!(state.update(&mut ctx), Running);
    ctx.urgent = true;
    assert_eq!(state.update(&mut ctx), Success);
    assert_eq!(ctx.scans, 1);
    assert_eq!(state.update(&mut ctx), Success);
    assert_eq!(ctx.scans, 2);
}

#[derive(Default)]
struct OwnedState {
    drops: Option<Arc<AtomicUsize>>,
}

impl Drop for OwnedState {
    fn drop(&mut self) {
        if let Some(drops) = &self.drops {
            drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

struct SuspendOnce(Arc<AtomicUsize>);

impl BtNode<Vec<EntryMode>> for SuspendOnce {
    type State = OwnedState;

    fn update(
        &self,
        state: &mut OwnedState,
        modes: &mut Vec<EntryMode>,
        _: &mut ExecutionCursor<'_>,
        mode: EntryMode,
    ) -> NodeResult {
        modes.push(mode);
        if state.drops.is_none() {
            state.drops = Some(self.0.clone());
            Running
        } else {
            Success
        }
    }
}

#[test]
fn repeated_child_gets_fresh_state_after_terminal_result() {
    let drops = Arc::new(AtomicUsize::new(0));
    let tree = control(Repeat(2), (SuspendOnce(drops.clone()),));
    let mut state = BtState::new(&tree);
    let mut modes = vec![];
    assert_eq!(state.update(&mut modes), Running);
    // Complete the first child and start its next invocation in the same update.
    assert_eq!(state.update(&mut modes), Running);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(state.update(&mut modes), Success);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(
        modes,
        [
            EntryMode::Evaluate,
            EntryMode::Resume,
            EntryMode::Evaluate,
            EntryMode::Resume
        ]
    );
}

#[test]
fn separate_instances_own_and_drop_their_suspended_state() {
    let drops = Arc::new(AtomicUsize::new(0));
    let tree = seq((SuspendOnce(drops.clone()),));
    let mut first = BtState::new(&tree);
    let mut second = BtState::new(&tree);
    let mut modes = vec![];
    assert_eq!(first.update(&mut modes), Running);
    assert_eq!(second.update(&mut modes), Running);
    first.reset();
    assert!(!first.is_running());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(second);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(first.update(&mut modes), Running);
    drop(first);
    assert_eq!(drops.load(Ordering::Relaxed), 3);
}

#[test]
fn failure_after_suspension_releases_path_and_runs_fallback() {
    let drops = Arc::new(AtomicUsize::new(0));
    let tree = select((
        seq((
            SuspendOnce(drops.clone()),
            leaf(|_: &mut Vec<EntryMode>| Failure),
        )),
        leaf(|modes: &mut Vec<EntryMode>| {
            modes.clear();
            Success
        }),
    ));
    let mut state = BtState::new(&tree);
    let mut modes = vec![];
    assert_eq!(state.update(&mut modes), Running);
    assert_eq!(state.update(&mut modes), Success);
    assert!(modes.is_empty());
    assert!(!state.is_running());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}
