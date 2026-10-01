//! Traces record only with debug assertions.
#![cfg(debug_assertions)]

use flatbt::inspect::{Inspector, NodeInfo};
use flatbt::prelude::*;
use flatbt::trace::{TraceLog, trace};

fn has_ammo(ammo: &u32) -> bool {
    *ammo > 0
}

fn tree() -> impl BtNode<u32, &'static str> {
    select((
        named(
            "attack",
            scope! {
                let burst: u32 = |ammo: &mut u32| (*ammo).min(3);
                sequence {
                    check(has_ammo);
                    leaf(|_: &mut u32| NodeResult::Running("fire"));
                }
            },
        ),
        choose!(|ammo: &u32| match *ammo {
            0 => named("reload", leaf(|_: &mut u32| NodeResult::Running("reload"))),
            _ => leaf(|_: &mut u32| NodeResult::Running("wait")),
        }),
    ))
}

#[test]
fn a_rejected_branch_shows_the_node_that_failed_it() {
    let tree = tree();
    let mut state = BtState::new(&tree);
    let log = TraceLog::new();
    let _ = update(&tree, &mut state, &mut 0, log.entry(EntryMode::Evaluate));
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "select {next: [RunChild(0), RunChild(1)]} → Running\n\
         \x20 attack (scope) → Failure\n\
         \x20   burst (compute) → Success\n\
         \x20   seq {next: [RunChild(0), Failure]} → Failure\n\
         \x20     has_ammo (check) → Failure    ← cause\n\
         \x20 choose {next: RunChild(0)} → Running\n\
         \x20   0 => reload (leaf) → Running"
    );
    assert_eq!(
        state.trace(&log).to_string(),
        "select > choose > 0 => reload (leaf)"
    );
}

#[test]
fn resume_enters_only_the_saved_path_and_says_so() {
    let tree = tree();
    let mut state = BtState::new(&tree);
    let log = TraceLog::new();
    let _ = update(&tree, &mut state, &mut 0, log.entry(EntryMode::Evaluate));
    let _ = update(&tree, &mut state, &mut 0, log.entry(EntryMode::Resume));
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "select (resume) → Running\n\
         \x20 choose (resume) → Running\n\
         \x20   0 => reload (leaf) (resume) → Running"
    );
}

#[test]
fn the_running_path_shows_its_live_fields() {
    let tree = tree();
    let mut state = BtState::new(&tree);
    let log = TraceLog::new();
    let _ = update(&tree, &mut state, &mut 5, log.entry(EntryMode::Evaluate));
    assert_eq!(
        state.trace(&log).to_string(),
        "select > attack (scope) {burst: 3} > seq > leaf"
    );
}

#[test]
fn a_tree_that_failed_outright_is_still_traced() {
    let tree = seq((check(has_ammo), leaf(|_: &mut u32| NodeResult::RUNNING)));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let log = TraceLog::new();
    assert_eq!(
        update(&tree, &mut state, &mut 0, log.entry(EntryMode::Evaluate)),
        NodeResult::Failure
    );
    assert!(!state.is_running());
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "seq {next: [RunChild(0), Failure]} → Failure\n\
         \x20 has_ammo (check) → Failure    ← cause"
    );
}

#[test]
fn a_node_entered_several_times_lists_each_outcome() {
    let tree = repeat(
        2,
        guard(
            |n: &u32| *n < 10,
            leaf(|n: &mut u32| {
                *n += 1;
                NodeResult::Success
            }),
        ),
    );
    let mut state: BtState<_, _> = BtState::new(&tree);
    let log = TraceLog::new();
    let _ = update(&tree, &mut state, &mut 9, log.entry(EntryMode::Evaluate));
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "repeat {times: 2, next: [RunChild(0), RunChild(0), Failure]} → Failure\n\
         \x20 guard → Success {if: true}, Failure {if: false}    ← cause\n\
         \x20   leaf → Success"
    );
}

#[test]
fn an_update_without_the_log_leaves_it_as_it_was() {
    let tree = tree();
    let mut state = BtState::new(&tree);
    let log = TraceLog::new();
    let _ = update(&tree, &mut state, &mut 0, log.entry(EntryMode::Evaluate));
    let _ = update(&tree, &mut state, &mut 0, EntryMode::Resume);
    assert!(
        format!("{:#}", state.trace(&log))
            .starts_with("select {next: [RunChild(0), RunChild(1)]} → Running\n")
    );
}

#[test]
fn a_slot_driver_keeps_its_own_log_with_a_limit() {
    let tree = tree();
    let (mut slot, mut memory) = (None, Default::default());
    let log = TraceLog::with_limit(2);
    let _ = update_slot(
        &tree,
        &mut slot,
        &mut memory,
        &mut 0,
        log.entry(EntryMode::Evaluate),
    );
    assert!(log.overflowed());
    // The limit counts calls and recorded values together.
    assert!(log.calls().count() <= 2);
    assert!(
        format!(
            "{:#}",
            trace::<u32, &str, _>(&tree, slot.as_ref(), &memory, &log)
        )
        .ends_with("… (limit reached)")
    );
}

/// Passes its entry on unchanged, as a node written before traces would.
struct PassThrough<N>(N);

impl<C, A, N: BtNode<C, A>> BtNode<C, A> for PassThrough<N> {
    type State = N::State;
    type Memory = N::Memory;

    fn update(
        &self,
        state: &mut N::State,
        memory: &mut N::Memory,
        ctx: &mut C,
        _: (),
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        self.0.update(state, memory, ctx, (), entry)
    }
}

/// Calls its child through its entry and reports it, so traces see inside.
struct Wrapped<N>(N);

impl<C, A, N: BtNode<C, A>> BtNode<C, A> for Wrapped<N> {
    type State = N::State;
    type Memory = N::Memory;
    const NODES: usize = 1 + N::NODES;

    fn update(
        &self,
        state: &mut N::State,
        memory: &mut N::Memory,
        ctx: &mut C,
        _: (),
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        entry.run(1, &self.0, state, memory, ctx, ())
    }

    fn inspect(&self, state: Option<&N::State>, memory: &N::Memory, inspector: &mut dyn Inspector) {
        inspector.node(NodeInfo::new("wrapped", state.is_some()), |inspector| {
            self.0.inspect(state, memory, inspector)
        });
    }
}

fn failing_branch() -> impl BtNode<u32, &'static str> {
    seq((
        check(has_ammo),
        leaf(|_: &mut u32| NodeResult::Running("fire")),
    ))
}

fn fallback() -> impl BtNode<u32, &'static str> {
    named("reload", leaf(|_: &mut u32| NodeResult::Running("reload")))
}

#[test]
fn a_node_passing_its_entry_on_is_traced_as_one_node() {
    let tree = select((PassThrough(failing_branch()), fallback()));
    let mut state = BtState::new(&tree);
    let log = TraceLog::new();
    let _ = update(&tree, &mut state, &mut 0, log.entry(EntryMode::Evaluate));
    // Nothing from inside it lands on `reload`, the next node in preorder. Its
    // child ran under its entry, so the child's answers show as its own.
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "select {next: [RunChild(0), RunChild(1)]} → Running\n\
         \x20 PassThrough {next: [RunChild(0), Failure]} → Failure    ← cause\n\
         \x20 reload (leaf) → Running"
    );
}

#[test]
fn a_node_running_its_child_through_its_entry_is_traced_inside() {
    let tree = select((Wrapped(failing_branch()), fallback()));
    let mut state = BtState::new(&tree);
    let log = TraceLog::new();
    let _ = update(&tree, &mut state, &mut 0, log.entry(EntryMode::Evaluate));
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "select {next: [RunChild(0), RunChild(1)]} → Running\n\
         \x20 wrapped → Failure\n\
         \x20   seq {next: [RunChild(0), Failure]} → Failure\n\
         \x20     has_ammo (check) → Failure    ← cause\n\
         \x20 reload (leaf) → Running"
    );
}

#[test]
fn an_order_records_its_scores_and_picks() {
    struct Needs {
        hunger: f32,
        fatigue: f32,
    }
    let (score, options) = per_child!(|needs: &Needs| {
        needs.hunger => leaf(|_: &mut Needs| NodeResult::<&str>::Failure),
        needs.fatigue => leaf(|_: &mut Needs| NodeResult::Running("sleep")),
    });
    let tree = select(order_by(by_score(score), options));
    let mut state = BtState::new(&tree);
    let log = TraceLog::new();
    let mut needs = Needs {
        hunger: 0.9,
        fatigue: 0.5,
    };
    let _ = update(
        &tree,
        &mut state,
        &mut needs,
        log.entry(EntryMode::Evaluate),
    );
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "select {order: by_score, position: 1, tried: {0}, \
         next: [RunChild(0), RunChild(1)], score: [0.9, 0.5], pick: [0, 1]} → Running\n\
         \x20 needs.hunger => leaf → Failure    ← cause\n\
         \x20 needs.fatigue => leaf → Running"
    );
}

#[test]
fn actions_and_custom_nodes_record_through_their_entry() {
    struct Walk;
    impl BtAction<u32, &'static str> for Walk {
        type State = ();
        fn start(&self, _: &mut u32, _: ()) -> Option<()> {
            Some(())
        }
        fn is_in_progress(&self, _: &(), at: &u32, _: ()) -> bool {
            *at < 3
        }
        fn tick(&self, _: &mut (), at: &mut u32, _: ()) -> &'static str {
            *at += 1;
            "walk"
        }
    }
    struct Counted;
    impl BtNode<u32, &'static str> for Counted {
        type State = ();
        type Memory = ();
        fn update(
            &self,
            _: &mut (),
            _: &mut (),
            n: &mut u32,
            _: (),
            entry: Entry<'_>,
        ) -> NodeResult<&'static str> {
            entry.record("n", || *n);
            NodeResult::Success
        }
    }
    let tree = seq((Counted, action(Walk)));
    let mut state = BtState::new(&tree);
    let log = TraceLog::new();
    let mut at = 2;
    let _ = update(&tree, &mut state, &mut at, log.entry(EntryMode::Resume));
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "seq {next: [RunChild(0), RunChild(1)]} → Running\n\
         \x20 Counted {n: 2} → Success\n\
         \x20 Walk (action) {started: true} → Running"
    );
    let _ = update(&tree, &mut state, &mut at, log.entry(EntryMode::Resume));
    assert_eq!(
        format!("{:#}", state.trace(&log)),
        "seq (resume) {next: Success} → Success\n\
         \x20 Walk (action) (resume) {completed: true} → Success"
    );
}

#[test]
fn a_diagnostic_is_recorded_on_the_node_that_raised_it() {
    let tree = scope! {
        let target: u32;
        sequence {
            leaf_with(|_: &mut u32, _: &u32| NodeResult::<()>::Success).with(target);
        }
    };
    let mut state: BtState<_, _> = BtState::new(&tree);
    let log = TraceLog::new();
    let _ = update(&tree, &mut state, &mut 0, log.entry(EntryMode::Evaluate));
    let text = format!("{:#}", state.trace(&log));
    assert!(
        text.contains("leaf_with {error: bound input is unavailable for "),
        "{text}"
    );
    assert!(text.ends_with("} → Failure    ← cause"), "{text}");
}

#[test]
fn tracing_scores_each_child_no_more_often() {
    use std::cell::Cell;

    struct Needs {
        scored: Cell<u32>,
    }
    fn tree() -> impl BtNode<Needs, &'static str> {
        select(order_by(
            by_score(|needs: &Needs, index: usize| {
                needs.scored.set(needs.scored.get() + 1);
                [0.9, 0.5][index]
            }),
            (
                leaf(|_: &mut Needs| NodeResult::<&str>::Failure),
                leaf(|_: &mut Needs| NodeResult::Running("sleep")),
            ),
        ))
    }
    let scored = |traced: bool| {
        let tree = tree();
        let mut state = BtState::new(&tree);
        let log = TraceLog::new();
        let mut needs = Needs {
            scored: Cell::new(0),
        };
        let entry = if traced {
            log.entry(EntryMode::Evaluate)
        } else {
            EntryMode::Evaluate.into()
        };
        let _ = update(&tree, &mut state, &mut needs, entry);
        needs.scored.get()
    };
    assert_eq!(scored(true), scored(false));
}
