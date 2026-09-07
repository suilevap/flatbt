use flatbt::{
    BtControl, BtNode, BtState, ControlOp, NodeResult, check, control, leaf, select, seq,
};

#[path = "../examples/support/mod.rs"]
mod support;
use NodeResult::{Failure, Success};
use support::Repeat;

fn record(label: &'static str, result: NodeResult) -> impl BtNode<Vec<&'static str>> {
    leaf(move |trace: &mut Vec<&'static str>| {
        trace.push(label);
        result
    })
}

#[test]
fn sequence_runs_in_order_and_stops_on_failure() {
    let mut trace = vec![];
    let tree = seq((
        record("a", Success),
        record("b", Failure),
        record("c", Success),
    ));
    assert_eq!(BtState::new(&tree).update(&mut trace), Failure);
    assert_eq!(trace, ["a", "b"]);
}

#[test]
fn selector_uses_nested_fallback_and_stops_on_success() {
    let mut trace = vec![];
    let tree = select((
        seq((check(|_: &Vec<&str>| false), record("unreachable", Success))),
        seq((record("a", Success), record("b", Success))),
        record("unused", Success),
    ));
    assert_eq!(BtState::new(&tree).update(&mut trace), Success);
    assert_eq!(trace, ["a", "b"]);
}

#[test]
fn selector_fails_when_all_children_fail() {
    let mut trace = vec![];
    let tree = select((record("a", Failure), record("b", Failure)));
    assert_eq!(BtState::new(&tree).update(&mut trace), Failure);
    assert_eq!(trace, ["a", "b"]);
}

#[test]
fn empty_controls_have_identity_results() {
    assert_eq!(BtState::new(&seq(())).update(&mut ()), Success);
    assert_eq!(BtState::new(&select(())).update(&mut ()), Failure);
}

#[test]
fn custom_policy_has_fresh_state_after_completion() {
    let tree = control(Repeat(3), (record("repeat", Success),));
    let mut state = BtState::new(&tree);
    let mut trace = vec![];
    assert_eq!(state.update(&mut trace), Success);
    assert_eq!(state.update(&mut trace), Success);
    assert_eq!(trace, ["repeat"; 6]);
}

#[test]
fn execution_errors_fail_the_branch_and_allow_fallback() {
    struct InvalidIndex;
    impl<C> BtControl<C> for InvalidIndex {
        type State = ();
        fn begin(&self, _: &mut (), _: &mut C, count: usize) -> ControlOp {
            ControlOp::RunChild(count)
        }
        fn child_succeeded(&self, _: &mut (), _: &mut C, _: usize, _: usize) -> ControlOp {
            ControlOp::Success
        }
        fn child_failed(&self, _: &mut (), _: &mut C, _: usize, _: usize) -> ControlOp {
            ControlOp::Failure
        }
    }

    let tree = select((
        control(InvalidIndex, (record("unreachable", Success),)),
        control(Repeat(1), ()), // Reports a custom policy configuration error.
        leaf(|_: &mut Vec<&str>| NodeResult::error("custom leaf could not execute")),
        record("fallback", Success),
    ));
    let mut state = BtState::new(&tree);
    let mut trace = vec![];
    assert_eq!(state.update(&mut trace), Success);
    assert_eq!(trace, ["fallback"]);
    assert!(!state.is_running());
}
