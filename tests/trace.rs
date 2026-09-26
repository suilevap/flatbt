//! Traces record only with debug assertions.
#![cfg(debug_assertions)]

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
        "select → Running\n\
         \x20 attack (scope) → Failure\n\
         \x20   burst (compute) → Success\n\
         \x20   seq → Failure\n\
         \x20     has_ammo (check) → Failure    ← cause\n\
         \x20 choose → Running\n\
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
        "seq → Failure\n\
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
        "repeat {times: 2} → Failure\n\
         \x20 guard → Success, Failure    ← cause\n\
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
    assert!(format!("{:#}", state.trace(&log)).starts_with("select → Running\n"));
}

#[test]
fn a_slot_driver_keeps_its_own_log_with_a_limit() {
    let tree = tree();
    let mut slot = None;
    let log = TraceLog::with_limit(2);
    let _ = update_slot(&tree, &mut slot, &mut 0, log.entry(EntryMode::Evaluate));
    assert!(log.overflowed());
    assert_eq!(log.calls().count(), 2);
    assert!(
        format!("{:#}", trace::<u32, &str, _>(&tree, slot.as_ref(), &log))
            .ends_with("… (limit reached)")
    );
}
