use flatbt::{
    BtChildren, BtControl, BtNode, ControlOp, NodeResult, check, control, leaf, select, seq,
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
    assert_eq!(tree.update(&mut trace), Failure);
    assert_eq!(trace, ["a", "b"]);
}

#[test]
fn sequence_succeeds_after_all_children() {
    let mut trace = vec![];
    assert_eq!(
        seq((record("a", Success), record("b", Success))).update(&mut trace),
        Success
    );
    assert_eq!(trace, ["a", "b"]);
}

#[test]
fn selector_tries_priorities_and_stops_on_success() {
    let mut trace = vec![];
    let tree = select((
        record("a", Failure),
        record("b", Success),
        record("c", Success),
    ));
    assert_eq!(tree.update(&mut trace), Success);
    assert_eq!(trace, ["a", "b"]);
}

#[test]
fn selector_fails_after_all_children() {
    let mut trace = vec![];
    assert_eq!(
        select((record("a", Failure), record("b", Failure))).update(&mut trace),
        Failure
    );
    assert_eq!(trace, ["a", "b"]);
}

#[test]
fn empty_controls_have_identity_results() {
    assert_eq!(seq(()).update(&mut ()), Success);
    assert_eq!(select(()).update(&mut ()), Failure);
}

#[test]
fn nested_heterogeneous_children_use_fallback() {
    let mut trace = vec![];
    let owned_label = String::from("owned configuration");
    let tree = select((
        seq((check(|_: &Vec<&str>| false), record("unreachable", Success))),
        seq((
            leaf(move |_: &mut Vec<&str>| {
                assert_eq!(owned_label, "owned configuration");
                Success
            }),
            record("fallback", Success),
        )),
    ));
    assert_eq!(tree.update(&mut trace), Success);
    assert_eq!(trace, ["fallback"]);
}

#[test]
fn each_update_restarts_the_tree_and_uses_current_context() {
    let tree = select((
        seq((
            check(|ctx: &usize| *ctx > 0),
            leaf(|ctx: &mut usize| {
                *ctx -= 1;
                Success
            }),
        )),
        leaf(|_: &mut usize| Failure),
    ));
    let mut ctx = 1;
    assert_eq!(tree.update(&mut ctx), Success);
    assert_eq!(tree.update(&mut ctx), Failure);
    let mut another_ctx = 2;
    assert_eq!(tree.update(&mut another_ctx), Success);
    assert_eq!(another_ctx, 1);
}

#[test]
fn custom_policy_repeats_child_and_has_fresh_state_per_invocation() {
    let tree = control(Repeat(3), (record("repeat", Success),));
    let mut trace = vec![];
    assert_eq!(tree.update(&mut trace), Success);
    assert_eq!(tree.update(&mut trace), Success);
    assert_eq!(trace, ["repeat"; 6]);
}

#[test]
fn custom_policy_stops_on_failure_and_zero_repeats_skip_child() {
    let mut trace = vec![];
    assert_eq!(
        control(Repeat(3), (record("failed", Failure),)).update(&mut trace),
        Failure
    );
    assert_eq!(
        control(Repeat(0), (record("skipped", Failure),)).update(&mut trace),
        Success
    );
    assert_eq!(trace, ["failed"]);
}

struct InvalidPolicy;
impl BtControl<()> for InvalidPolicy {
    type State = ();
    fn begin(&self, _: &mut (), _: &mut (), count: usize) -> ControlOp {
        ControlOp::RunChild(count)
    }
    fn child_succeeded(&self, _: &mut (), _: &mut (), _: usize, _: usize) -> ControlOp {
        unreachable!()
    }
    fn child_failed(&self, _: &mut (), _: &mut (), _: usize, _: usize) -> ControlOp {
        unreachable!()
    }
}

#[test]
#[should_panic(expected = "control policy returned invalid child index 1 for 1 children")]
fn invalid_custom_policy_is_rejected_before_dispatch() {
    let _ = control(InvalidPolicy, (leaf(|_: &mut ()| Success),)).update(&mut ());
}

#[test]
#[should_panic(expected = "out of bounds")]
fn direct_tuple_dispatch_checks_bounds() {
    let _ = (record("unused", Success),).run_child(1, &mut vec![]);
}

#[test]
fn largest_supported_tuple_dispatches_every_position() {
    struct Index(usize);
    impl BtNode<Vec<usize>> for Index {
        fn update(&self, trace: &mut Vec<usize>) -> NodeResult {
            trace.push(self.0);
            Success
        }
    }
    let tree = seq((
        Index(0),
        Index(1),
        Index(2),
        Index(3),
        Index(4),
        Index(5),
        Index(6),
        Index(7),
        Index(8),
        Index(9),
        Index(10),
        Index(11),
        Index(12),
        Index(13),
        Index(14),
        Index(15),
        Index(16),
        Index(17),
        Index(18),
        Index(19),
        Index(20),
        Index(21),
        Index(22),
        Index(23),
        Index(24),
        Index(25),
        Index(26),
        Index(27),
        Index(28),
        Index(29),
        Index(30),
        Index(31),
    ));
    let mut trace = vec![];
    assert_eq!(tree.update(&mut trace), Success);
    assert_eq!(trace, (0..32).collect::<Vec<_>>());
}
