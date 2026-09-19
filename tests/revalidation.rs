use flatbt::{BtState, update};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use NodeResult::{Failure, Success};

/// `Running` for a tree that decides nothing; see `NodeResult::RUNNING`.
#[allow(non_upper_case_globals)]
const Running: NodeResult = NodeResult::RUNNING;

use flatbt::{BtNode, EntryMode, NodeResult, leaf, select, seq};

#[path = "../examples/support/wait_frames.rs"]
mod wait;
use wait::wait_frames;

#[test]
fn evaluate_preserves_sequence_progress_but_rescans_selector() {
    struct Context {
        gate_open: bool,
        checks: usize,
    }
    fn gate(ctx: &mut Context) -> NodeResult {
        ctx.checks += 1;
        if ctx.gate_open { Success } else { Failure }
    }

    let sequence = seq((leaf(gate), wait_frames(1)));
    let mut state: BtState<_, _> = BtState::new(&sequence);
    let mut ctx = Context {
        gate_open: true,
        checks: 0,
    };
    assert_eq!(
        update(&sequence, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    ctx.gate_open = false;
    assert_eq!(
        update(&sequence, &mut state, &mut ctx, EntryMode::Evaluate),
        Success
    );
    assert_eq!(ctx.checks, 1);

    let selector = select((leaf(gate), wait_frames(1)));
    let mut state: BtState<_, _> = BtState::new(&selector);
    let mut ctx = Context {
        gate_open: false,
        checks: 0,
    };
    assert_eq!(
        update(&selector, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    ctx.gate_open = true;
    assert_eq!(
        update(&selector, &mut state, &mut ctx, EntryMode::Evaluate),
        Success
    );
    assert_eq!(ctx.checks, 2);
}

#[derive(Default)]
struct Context {
    urgent: bool,
    entries: Vec<(&'static str, EntryMode, usize)>,
}

struct Probe {
    name: &'static str,
    suspend_updates: usize,
    terminal: NodeResult,
    drops: Arc<AtomicUsize>,
}

#[derive(Default)]
struct ProbeState {
    updates: usize,
    drops: Option<Arc<AtomicUsize>>,
}

impl Drop for ProbeState {
    fn drop(&mut self) {
        if let Some(drops) = &self.drops {
            drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl BtNode<Context> for Probe {
    type State = ProbeState;

    fn update(
        &self,
        state: &mut ProbeState,
        ctx: &mut Context,
        _: (),
        mode: EntryMode,
    ) -> NodeResult {
        state.drops.get_or_insert_with(|| self.drops.clone());
        state.updates += 1;
        ctx.entries.push((self.name, mode, state.updates));
        if state.updates <= self.suspend_updates {
            Running
        } else {
            self.terminal
        }
    }
}

#[test]
fn failed_candidate_preserves_existing_state_for_evaluate() {
    let candidate_drops = Arc::new(AtomicUsize::new(0));
    let old_drops = Arc::new(AtomicUsize::new(0));
    let tree = select((
        Probe {
            name: "candidate",
            suspend_updates: 0,
            terminal: Failure,
            drops: candidate_drops.clone(),
        },
        Probe {
            name: "old",
            suspend_updates: 1,
            terminal: Success,
            drops: old_drops.clone(),
        },
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut ctx = Context::default();
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate),
        Success
    );
    assert_eq!(
        ctx.entries,
        [
            ("candidate", EntryMode::Evaluate, 1),
            ("old", EntryMode::Evaluate, 1),
            ("candidate", EntryMode::Evaluate, 1),
            ("old", EntryMode::Evaluate, 2),
        ]
    );
    assert_eq!(candidate_drops.load(Ordering::Relaxed), 2);
    assert_eq!(old_drops.load(Ordering::Relaxed), 1);
    assert!(!state.is_running());
}

#[test]
fn higher_priority_running_candidate_preempts_and_drops_old_path() {
    let old_drops = Arc::new(AtomicUsize::new(0));
    let new_drops = Arc::new(AtomicUsize::new(0));
    let tree = select((
        seq((
            leaf(|ctx: &mut Context| if ctx.urgent { Success } else { Failure }),
            Probe {
                name: "new",
                suspend_updates: 3,
                terminal: Success,
                drops: new_drops.clone(),
            },
        )),
        Probe {
            name: "old",
            suspend_updates: 3,
            terminal: Success,
            drops: old_drops.clone(),
        },
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut ctx = Context::default();
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    ctx.urgent = true;
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate),
        Running
    );
    assert_eq!(old_drops.load(Ordering::Relaxed), 1);
    assert_eq!(new_drops.load(Ordering::Relaxed), 0);
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    assert_eq!(
        ctx.entries,
        [
            ("old", EntryMode::Evaluate, 1),
            ("new", EntryMode::Evaluate, 1),
            ("new", EntryMode::Resume, 2),
        ]
    );
    state.reset();
    assert_eq!(new_drops.load(Ordering::Relaxed), 1);
}
