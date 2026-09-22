use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use EntryMode::{Evaluate, Resume};
use NodeResult::{Failure, Success};

/// `Running` for a tree that decides nothing; see `NodeResult::RUNNING`.
#[allow(non_upper_case_globals)]
const Running: NodeResult = NodeResult::RUNNING;
use flatbt::{BtNode, BtState, EntryMode, NodeResult, choose, leaf, select, seq, update};

#[derive(Default)]
struct Context {
    order: usize,
    inner: bool,
    entries: Vec<(&'static str, EntryMode, usize)>,
}

struct Probe {
    name: &'static str,
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
        state: &mut Self::State,
        ctx: &mut Context,
        _: (),
        mode: EntryMode,
    ) -> NodeResult {
        state.drops.get_or_insert_with(|| self.drops.clone());
        state.updates += 1;
        ctx.entries.push((self.name, mode, state.updates));
        Running
    }
}

fn probe(name: &'static str, drops: &Arc<AtomicUsize>) -> Probe {
    Probe {
        name,
        drops: drops.clone(),
    }
}

#[test]
fn evaluate_changes_candidates_while_resume_keeps_the_saved_choice() {
    let a = Arc::new(AtomicUsize::new(0));
    let b = Arc::new(AtomicUsize::new(0));
    let tree = choose!(|bb: &Context| match bb.order {
        0 => probe("a", &a),
        _ => probe("b", &b),
    });
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut ctx = Context::default();
    assert_eq!(update(&tree, &mut state, &mut ctx, Resume), Running);
    assert_eq!(update(&tree, &mut state, &mut ctx, Evaluate), Running);
    ctx.order = 1;
    assert_eq!(update(&tree, &mut state, &mut ctx, Resume), Running);
    assert_eq!(update(&tree, &mut state, &mut ctx, Evaluate), Running);
    assert_eq!(a.load(Ordering::Relaxed), 1);
    assert_eq!(b.load(Ordering::Relaxed), 0);
    ctx.order = 0;
    assert_eq!(update(&tree, &mut state, &mut ctx, Evaluate), Running);
    assert_eq!(b.load(Ordering::Relaxed), 1);
    assert_eq!(
        ctx.entries,
        [
            ("a", Evaluate, 1),
            ("a", Evaluate, 2),
            ("a", Resume, 3),
            ("b", Evaluate, 1),
            ("a", Evaluate, 1),
        ]
    );
    state.reset();
    assert_eq!(a.load(Ordering::Relaxed), 2);
}

#[test]
fn nested_selection_replaces_only_the_selected_inner_state() {
    let a = Arc::new(AtomicUsize::new(0));
    let b = Arc::new(AtomicUsize::new(0));
    let tree = choose!(|bb: &Context| match bb.order {
        0 => seq((
            leaf(|ctx: &mut Context| {
                ctx.entries.push(("prefix", Evaluate, 1));
                Success
            }),
            choose!(|bb: &Context| match bb.inner {
                false => probe("a", &a),
                true => seq((probe("b", &b),)),
            }),
        )),
        _ => leaf(|_: &mut Context| Success),
    });
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut ctx = Context::default();
    assert_eq!(update(&tree, &mut state, &mut ctx, Resume), Running);
    ctx.inner = true;
    assert_eq!(update(&tree, &mut state, &mut ctx, Evaluate), Running);
    assert_eq!(a.load(Ordering::Relaxed), 1);
    assert_eq!(update(&tree, &mut state, &mut ctx, Resume), Running);
    assert_eq!(
        ctx.entries,
        [
            ("prefix", Evaluate, 1),
            ("a", Evaluate, 1),
            ("b", Evaluate, 1),
            ("b", Resume, 2),
        ]
    );
    ctx.order = 1;
    assert_eq!(update(&tree, &mut state, &mut ctx, Evaluate), Success);
    assert!(!state.is_running());
    assert_eq!(b.load(Ordering::Relaxed), 1);
}

#[test]
fn terminal_choice_is_forwarded_without_fallback_and_releases_the_old_state() {
    for terminal in [Success, Failure] {
        let drops = Arc::new(AtomicUsize::new(0));
        let tree = choose!(|bb: &Context| match bb.order {
            0 => probe("old", &drops),
            1 => leaf(|_: &mut Context| terminal),
            _ => leaf(|ctx: &mut Context| {
                ctx.entries.push(("unselected", Evaluate, 1));
                Success
            }),
        });
        let mut state: BtState<_, _> = BtState::new(&tree);
        let mut ctx = Context::default();
        assert_eq!(update(&tree, &mut state, &mut ctx, Resume), Running);
        ctx.order = 1;
        assert_eq!(update(&tree, &mut state, &mut ctx, Evaluate), terminal);
        assert!(!state.is_running());
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert_eq!(ctx.entries, [("old", Evaluate, 1)]);
    }
}

#[path = "support/reject.rs"]
mod reject;
use reject::Reject;

#[test]
fn outer_rejection_drops_nested_candidates_and_preserves_the_saved_branch() {
    let candidate = Arc::new(AtomicUsize::new(0));
    let saved = Arc::new(AtomicUsize::new(0));
    let tree = select((
        Reject(choose!(|bb: &Context| match bb.order {
            0 => choose!(|bb: &Context| match bb.inner {
                _ => probe("candidate", &candidate),
            }),
            _ => leaf(|_: &mut Context| Failure),
        })),
        probe("saved", &saved),
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut ctx = Context::default();
    assert_eq!(update(&tree, &mut state, &mut ctx, Resume), Running);
    assert_eq!(update(&tree, &mut state, &mut ctx, Evaluate), Running);
    assert_eq!(candidate.load(Ordering::Relaxed), 2);
    assert_eq!(saved.load(Ordering::Relaxed), 0);
    assert_eq!(
        ctx.entries,
        [
            ("candidate", Evaluate, 1),
            ("saved", Evaluate, 1),
            ("candidate", Evaluate, 1),
            ("saved", Evaluate, 2),
        ]
    );
    drop(state);
    assert_eq!(saved.load(Ordering::Relaxed), 1);
}

#[test]
fn definitions_are_built_once_and_chooser_supports_guards_and_owned_captures() {
    let mut constructions = Vec::new();
    // An owned, non-Copy capture must survive after construction.
    let allowed: Vec<_> = (2..=3).collect();
    let tree = choose!(move |bb: &Context| match bb.order.checked_add(0) {
        Some(order) if allowed.contains(&order) => {
            constructions.push("allowed");
            leaf(|_: &mut Context| Success)
        }
        Some(0 | 1) | None => {
            constructions.push("small");
            leaf(|_: &mut Context| Failure)
        }
        _ => {
            constructions.push("other");
            leaf(|_: &mut Context| Running)
        }
    });
    assert_eq!(constructions, ["allowed", "small", "other"]);
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut ctx = Context::default();
    for (order, result) in [(0, Failure), (2, Success), (4, Running), (3, Success)] {
        ctx.order = order;
        assert_eq!(update(&tree, &mut state, &mut ctx, Evaluate), result);
    }
    assert_eq!(constructions.len(), 3);
}
