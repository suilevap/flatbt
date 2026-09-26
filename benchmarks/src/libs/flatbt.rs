use crate::common::{Bb, Op, Outcome, Scenario};
use crate::harness::{Measure, entry};
use crate::soldier::{Abort::*, Cond::*, Effect::*, Place, SoldierOp};
use flatbt::prelude::*;

pub const NAME: &str = "flatbt";
/// Every update enters with `Evaluate`: selectors rescan from their first
/// child, as bt-tree (C#) does. Used only for that comparison.
pub const EVALUATE: &str = "flatbt-evaluate";

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

fn c(cond: crate::soldier::Cond) -> impl BtNode<Bb> {
    op(SoldierOp::If(cond).into())
}

fn go(place: Place, yields: bool) -> impl BtNode<Bb> {
    op(SoldierOp::Goto(place, yields).into())
}

fn work(
    slot: u8,
    ticks: u8,
    abort: crate::soldier::Abort,
    effect: crate::soldier::Effect,
) -> impl BtNode<Bb> {
    op(SoldierOp::Work(slot, ticks, abort, effect).into())
}

fn act(effect: crate::soldier::Effect) -> impl BtNode<Bb> {
    op(SoldierOp::Do(effect).into())
}

/// The same tree as `soldier::spec`.
fn soldier() -> impl BtNode<Bb> {
    select((
        seq((c(Dead), work(0, 20, Never, Respawn))),
        seq((
            c(LowHealth),
            select((
                seq((c(HasMedkit), work(1, 3, Never, UseMedkit))),
                seq((
                    c(FoeVisible),
                    go(Place::Cover, false),
                    work(2, 6, Never, Heal(25)),
                )),
                seq((go(Place::Base, false), work(2, 6, Never, Heal(25)))),
            )),
        )),
        seq((
            c(FoeVisible),
            select((
                seq((
                    c(MagEmpty),
                    select((
                        seq((c(HasReserve), work(3, 4, Never, Reload))),
                        seq((c(FoeClose), work(4, 3, OnFoeGone, Melee))),
                        seq((go(Place::Ammo, false), act(TakeAmmo))),
                    )),
                )),
                seq((
                    c(FoeInRange),
                    select((
                        seq((c(HasGrenade), c(FoeGrouped), act(Throw))),
                        seq((work(5, 2, OnFoeGone, Idle), act(Fire))),
                    )),
                )),
                op(SoldierOp::Close(12).into()),
            )),
        )),
        seq((
            c(HeardNoise),
            go(Place::Noise, true),
            work(6, 3, OnFoe, Idle),
            act(ClearNoise),
        )),
        seq((
            c(Hungry),
            select((
                seq((c(HasFood), work(7, 5, OnFoe, Eat))),
                seq((go(Place::Kitchen, true), act(TakeFood))),
            )),
        )),
        seq((c(Tired), go(Place::Bed, true), work(8, 12, OnFoe, Sleep))),
        seq((
            go(Place::At(12), true),
            work(6, 3, OnFoe, Idle),
            go(Place::At(-12), true),
            work(6, 3, OnFoe, Idle),
            go(Place::At(0), true),
            act(Score(1)),
        )),
    ))
}

#[cfg(feature = "catalog")]
fn v(villager_op: crate::villager::VillagerOp) -> impl BtNode<Bb> {
    op(villager_op.into())
}

/// The same tree as `villager::spec`, with the node catalog.
#[cfg(feature = "catalog")]
fn villager() -> impl BtNode<Bb> {
    use crate::villager::{
        Abort::*, Cond as If, Effect as E, Place as At, VillagerOp::*, draw, healthy, tired,
    };
    let work = |slot, ticks, abort, effect| v(Work(slot, ticks, abort, effect));
    let chat = |i| {
        seq((
            v(If(If::FriendHome(i))),
            v(Goto(At::Friend(i), true)),
            work(4, 3, OnThreat, E::Chat(i)),
        ))
    };
    let chore = |place, effect| seq((v(Goto(place, true)), work(5, 2, OnThreat, effect)));
    let (need, needs) = per_child!(|bb: &Bb| {
        bb.villager.hunger => seq((
            retry(3, v(Forage)),
            v(Goto(At::Kitchen, true)),
            repeat(2, work(2, 3, OnThreat, E::Chew)),
            v(Do(E::Eat)),
        )),
        bb.villager.fatigue => seq((
            v(Goto(At::Bed, true)),
            repeat_while(tired, work(3, 2, OnThreat, E::Rest)),
        )),
        bb.villager.boredom => {
            let (weight, friends) = per_child!(|bb: &Bb| {
                1.0 + bb.villager.friendship[0].min(20) as f32 => chat(0),
                1.0 + bb.villager.friendship[1].min(20) as f32 => chat(1),
                1.0 + bb.villager.friendship[2].min(20) as f32 => chat(2),
            });
            weighted_select(draw, weight, friends)
        },
        bb.villager.duty => shuffle_seq(
            draw,
            (
                chore(At::Woodpile, E::Chop),
                chore(At::Well, E::Carry),
                chore(At::Hall, E::Sweep),
            ),
        ),
    });
    select((
        seq((
            v(If(If::Threat)),
            if_else(
                healthy,
                seq((
                    repeat(3, work(0, 2, OnNoThreat, E::Swing)),
                    force_success(seq((v(If(If::Loot)), v(Do(E::TakeLoot))))),
                )),
                seq((
                    invert(v(If(If::Cornered))),
                    v(Goto(At::Safe, false)),
                    work(1, 4, Never, E::Hide),
                )),
            ),
        )),
        utility(need, needs),
    ))
}

/// One shared tree; each agent owns only `Option<Tree::State>`.
fn measure<N: BtNode<Bb> + 'static>(
    scenario: Scenario,
    tree: fn() -> N,
    mode: EntryMode,
) -> Box<dyn Measure>
where
    N::State: 'static,
{
    let name = match mode {
        EntryMode::Resume => NAME,
        EntryMode::Evaluate => EVALUATE,
    };
    entry(
        name,
        true,
        scenario,
        tree,
        |_: &N| None::<N::State>,
        move |tree, slot, bb| match update_slot(tree, slot, bb, mode) {
            NodeResult::Success => Outcome::Success,
            NodeResult::Failure => Outcome::Failure,
            NodeResult::Running(()) => Outcome::Running,
        },
    )
}

pub fn entries() -> Vec<Box<dyn Measure>> {
    with_mode(EntryMode::Resume)
}

pub fn evaluate_entries() -> Vec<Box<dyn Measure>> {
    with_mode(EntryMode::Evaluate)
}

fn with_mode(mode: EntryMode) -> Vec<Box<dyn Measure>> {
    vec![
        measure(Scenario::Select8, select8, mode),
        measure(Scenario::Patrol, patrol, mode),
        measure(Scenario::Guard, guard, mode),
        measure(Scenario::Soldier, soldier, mode),
        #[cfg(feature = "catalog")]
        measure(Scenario::Villager, villager, mode),
    ]
}
