use flatbt::{BtState, update};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flatbt::{BtNode, EntryMode, NodeResult, check, control, leaf, select, seq};

#[path = "../examples/support/mod.rs"]
mod support;
use NodeResult::{Failure, Success};

/// `Running` for a tree that decides nothing; see `NodeResult::RUNNING`.
#[allow(non_upper_case_globals)]
const Running: NodeResult = NodeResult::RUNNING;
use support::Repeat;

#[path = "../examples/support/wait_frames.rs"]
mod wait;
use wait::wait_frames;

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
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut trace = vec![];
    for _ in 0..3 {
        assert_eq!(
            update(&tree, &mut state, &mut trace, EntryMode::Resume),
            Running
        );
        assert_eq!(trace, ["check"]);
        assert!(state.is_running());
    }
    assert_eq!(
        update(&tree, &mut state, &mut trace, EntryMode::Resume),
        Success
    );
    assert_eq!(trace, ["check", "fire"]);
    assert!(!state.is_running());

    assert_eq!(
        update(&tree, &mut state, &mut trace, EntryMode::Resume),
        Running
    );
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
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut ctx = Context::default();
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    ctx.urgent = true;
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Success
    );
    assert_eq!(ctx.scans, 1);
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Success
    );
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
        _: (),
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
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut modes = vec![];
    assert_eq!(
        update(&tree, &mut state, &mut modes, EntryMode::Resume),
        Running
    );
    // Complete the first child and start its next invocation in the same update.
    assert_eq!(
        update(&tree, &mut state, &mut modes, EntryMode::Resume),
        Running
    );
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(
        update(&tree, &mut state, &mut modes, EntryMode::Resume),
        Success
    );
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
    let mut first: BtState<_, _> = BtState::new(&tree);
    let mut second: BtState<_, _> = BtState::new(&tree);
    let mut modes = vec![];
    assert_eq!(
        update(&tree, &mut first, &mut modes, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut second, &mut modes, EntryMode::Resume),
        Running
    );
    first.reset();
    assert!(!first.is_running());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(second);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(
        update(&tree, &mut first, &mut modes, EntryMode::Resume),
        Running
    );
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
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut modes = vec![];
    assert_eq!(
        update(&tree, &mut state, &mut modes, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut modes, EntryMode::Resume),
        Success
    );
    assert!(modes.is_empty());
    assert!(!state.is_running());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn a_resumed_branch_that_fails_falls_through_below_it_not_back_above() {
    // Order is priority, but Resume skips `begin()`: the branch above was not
    // consulted this update, and is not consulted when the resumed one fails
    // either. Resume means resume, so the fallthrough goes forward only, and
    // reconsidering is the caller's to ask for.
    #[derive(Default)]
    struct World {
        top_ready: bool,
        ran: Vec<&'static str>,
        middle_updates: u32,
    }

    let tree = select((
        seq((
            check(|w: &World| w.top_ready),
            leaf(|w: &mut World| {
                w.ran.push("top");
                Success
            }),
        )),
        leaf(|w: &mut World| {
            w.middle_updates += 1;
            w.ran.push("middle");
            if w.middle_updates >= 2 {
                Failure
            } else {
                Running
            }
        }),
        leaf(|w: &mut World| {
            w.ran.push("fallback");
            Success
        }),
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut world = World::default();

    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Evaluate),
        Running
    );
    world.top_ready = true;

    // The middle branch fails and the fallback below it runs, though the top
    // branch became available while it was suspended.
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Success
    );
    assert_eq!(world.ran, ["middle", "middle", "fallback"]);

    // That invocation ended, so the next one starts fresh and priority applies
    // again. A caller that wants this sooner re-enters with Evaluate itself --
    // which is what `flatbt_bevy::Behavior::tick` does on a failed resume.
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Success
    );
    assert_eq!(world.ran.last(), Some(&"top"));
}

#[test]
fn composed_state_drops_descendants_before_parents() {
    use std::sync::Mutex;

    type Trace = Arc<Mutex<Vec<&'static str>>>;
    struct Lease(&'static str, Trace);
    impl Drop for Lease {
        fn drop(&mut self) {
            self.1.lock().unwrap().push(self.0);
        }
    }
    struct Scope<N> {
        name: &'static str,
        child: N,
    }
    #[derive(Default)]
    struct ScopeState<S> {
        child: S,
        lease: Option<Lease>,
    }
    impl<N: BtNode<Trace>> BtNode<Trace> for Scope<N> {
        type State = ScopeState<N::State>;
        fn update(
            &self,
            state: &mut Self::State,
            trace: &mut Trace,
            _: (),
            mode: EntryMode,
        ) -> NodeResult {
            state
                .lease
                .get_or_insert_with(|| Lease(self.name, trace.clone()));
            self.child.update(&mut state.child, trace, (), mode)
        }
    }

    let tree = Scope {
        name: "parent",
        child: Scope {
            name: "child",
            child: leaf(|_: &mut Trace| Running),
        },
    };
    let mut trace = Trace::default();
    let mut state: BtState<_, _> = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut trace, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut trace, EntryMode::Evaluate),
        Running
    );
    assert!(trace.lock().unwrap().is_empty());
    drop(state);
    assert_eq!(*trace.lock().unwrap(), ["child", "parent"]);
}

#[test]
fn wrong_root_is_rejected_without_losing_the_saved_continuation() {
    let root = wait_frames(1);
    let other_root = wait_frames(5);
    let mut state: BtState<_, _> = BtState::new(&root);
    assert_eq!(
        update(&root, &mut state, &mut (), EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&other_root, &mut state, &mut (), EntryMode::Resume),
        Failure
    );
    assert!(state.is_running());
    assert_eq!(
        update(&root, &mut state, &mut (), EntryMode::Resume),
        Success
    );
}
