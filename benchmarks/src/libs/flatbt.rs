use crate::common::{Bb, Op, Outcome, Scenario};
use crate::harness::{Measure, entry};
use flatbt::prelude::*;

pub const NAME: &str = "flatbt";

fn op(op: Op) -> impl BtNode<Bb> {
    leaf(move |bb: &mut Bb| match op.run(bb) {
        Outcome::Success => NodeResult::Success,
        Outcome::Failure => NodeResult::Failure,
        Outcome::Running => NodeResult::RUNNING,
    })
}

fn branch(i: u8) -> impl BtNode<Bb> {
    seq((op(Op::IsMode(i)), op(Op::Act(i as u32))))
}

fn patrol() -> impl BtNode<Bb> {
    seq((
        op(Op::MoveTo(8)),
        op(Op::Act(1)),
        op(Op::MoveTo(-8)),
        op(Op::Act(2)),
    ))
}

fn select8() -> impl BtNode<Bb> {
    select((
        branch(0),
        branch(1),
        branch(2),
        branch(3),
        branch(4),
        branch(5),
        branch(6),
        branch(7),
    ))
}

fn guard() -> impl BtNode<Bb> {
    select((
        seq((op(Op::LowHp), op(Op::MoveTo(0)), op(Op::Heal))),
        seq((op(Op::EnemyNear), op(Op::Attack))),
        patrol(),
    ))
}

/// One shared tree; each agent owns only `Option<Tree::State>`.
fn measure<N: BtNode<Bb> + 'static>(scenario: Scenario, tree: fn() -> N) -> Box<dyn Measure>
where
    N::State: 'static,
{
    entry(
        NAME,
        true,
        scenario,
        tree,
        |_: &N| None::<N::State>,
        |tree, slot, bb| match update_slot(tree, slot, bb, EntryMode::Resume) {
            NodeResult::Success => Outcome::Success,
            NodeResult::Failure => Outcome::Failure,
            NodeResult::Running(()) => Outcome::Running,
        },
    )
}

pub fn entries() -> Vec<Box<dyn Measure>> {
    vec![
        measure(Scenario::Select8, select8),
        measure(Scenario::Patrol, patrol),
        measure(Scenario::Guard, guard),
    ]
}
