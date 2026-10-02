use crate::common::{BASIC, Bb, Op, Outcome, Spec};
use crate::harness::{Measure, entry};
use bonsai_bt::{ActionArgs, BT, Behavior, Event, RUNNING, Status, UpdateArgs};

pub const NAME: &str = "bonsai-bt";

fn build(spec: &Spec) -> Behavior<Op> {
    match spec {
        Spec::Seq(children) => Behavior::Sequence(children.iter().map(build).collect()),
        Spec::Sel(children) => Behavior::Select(children.iter().map(build).collect()),
        Spec::Leaf(op) => Behavior::Action(*op),
        _ => unreachable!("only BASIC scenarios"),
    }
}

/// `BT` owns its tree, so each agent clones the shared definition. A finished
/// `BT` ticks no further; `reset_bt` restarts it, as the crate documents.
fn tick(bt: &mut BT<Op, ()>, bb: &mut Bb) -> Outcome {
    let event: Event = UpdateArgs { dt: 1.0 }.into();
    let (status, _) = bt
        .tick(
            &event,
            &mut |args: ActionArgs<Event, Op>, _: &mut ()| match args.action.run(bb) {
                Outcome::Success => (Status::Success, args.dt),
                Outcome::Failure => (Status::Failure, args.dt),
                Outcome::Running => RUNNING,
            },
        )
        .expect("reset after every finish");
    if bt.is_finished() {
        bt.reset_bt();
    }
    match status {
        Status::Success => Outcome::Success,
        Status::Failure => Outcome::Failure,
        Status::Running => Outcome::Running,
    }
}

pub fn entries() -> Vec<Box<dyn Measure>> {
    // No custom composites: the enum of node kinds is closed.
    BASIC
        .into_iter()
        .map(|scenario| {
            entry(
                NAME,
                false,
                scenario,
                move || build(&scenario.spec()),
                |tree: &Behavior<Op>| BT::new(tree.clone(), ()),
                |_, bt, bb| tick(bt, bb),
            )
        })
        .collect()
}
