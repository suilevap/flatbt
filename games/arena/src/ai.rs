//! Three enemy minds over one blackboard.
//!
//! The point of the crate: whether one [`BehaviorContext`] really serves a
//! family of trees, and what an action does when it needs to ask the world
//! something the context never declared.

use core::time::Duration;

use bevy::ecs::query::QueryData;
use bevy::prelude::*;
use flatbt::bevy::prelude::*;
use flatbt::prelude::{BtAction, action, choose};

use crate::world::{Ammo, Arena, CoverTarget, Health, Speed, WantsCover};

/// What every enemy tree may touch. Declared once, for all three.
#[derive(QueryData)]
#[query_data(mutable)]
pub struct Fighter {
    pub transform: &'static mut Transform,
    pub health: &'static mut Health,
    pub ammo: &'static mut Ammo,
    pub speed: &'static Speed,
    /// Filled by `resolve_cover_requests`, a tick after a tree asks.
    pub cover: Option<&'static CoverTarget>,
}

/// How often a fighter is allowed to change its mind.
///
/// Not a detail: a tree that only ever resumes never leaves the branch it is
/// in, so `select` never rescans and `choose!` never re-picks. Every reactive
/// tree here -- the coward switching between fighting and hiding, `take_cover`
/// noticing that its request was answered -- depends on this. `evaluate_every`
/// staggers the agents so the whole population does not reconsider on one frame.
const RETHINK: Duration = Duration::from_millis(100);

impl BehaviorContext for Fighter {
    type Agent = Self;
    type Param = Res<'static, Arena>;

    fn entry_mode(bb: &Blackboard<Fighter>) -> EntryMode {
        evaluate_every(RETHINK, bb.shared.elapsed, bb.shared.delta, bb.entity)
    }
}

// --- shared pieces -----------------------------------------------------------

fn position(bb: &Blackboard<Fighter>) -> Vec2 {
    bb.transform.translation.truncate()
}

fn range_to_player(bb: &Blackboard<Fighter>) -> f32 {
    position(bb).distance(bb.shared.player)
}

fn step_towards(bb: &mut Blackboard<Fighter>, target: Vec2) {
    let from = position(bb);
    let step = (target - from).normalize_or_zero() * bb.speed.0;
    bb.transform.translation += step.extend(0.0);
}

fn hurt(bb: &Blackboard<Fighter>) -> bool {
    bb.health.0 < 40.0
}

fn has_ammo(bb: &Blackboard<Fighter>) -> bool {
    bb.ammo.0 > 0
}

fn shoot(bb: &mut Blackboard<Fighter>) -> NodeResult {
    bb.ammo.0 -= 1;
    NodeResult::Success
}

/// Reloading takes several ticks, and holds its progress across them.
struct Reload {
    ticks: u32,
    rounds: u32,
}

impl BtAction<Blackboard<'_, '_, '_, '_, '_, Fighter>> for Reload {
    type State = u32;

    fn start(&self, _: &mut Blackboard<'_, '_, '_, '_, '_, Fighter>, _: ()) -> Option<u32> {
        Some(0)
    }

    fn is_in_progress(
        &self,
        elapsed: &u32,
        _: &Blackboard<'_, '_, '_, '_, '_, Fighter>,
        _: (),
    ) -> bool {
        *elapsed < self.ticks
    }

    fn tick(&self, elapsed: &mut u32, _: &mut Blackboard<'_, '_, '_, '_, '_, Fighter>, _: ()) {
        *elapsed += 1;
    }

    fn complete(
        &self,
        _: &mut u32,
        bb: &mut Blackboard<'_, '_, '_, '_, '_, Fighter>,
        _: (),
    ) -> bool {
        bb.ammo.0 = self.rounds;
        true
    }
}

fn reload() -> impl BehaviorNode<Fighter> {
    seq((
        check(|bb: &Blackboard<Fighter>| !has_ammo(bb)),
        action(Reload {
            ticks: 30,
            rounds: 6,
        }),
    ))
}

/// The deferred query, as an action: ask once, wait, then use the answer.
///
/// A tree cannot run an arbitrary query itself -- the context declares its
/// access up front -- so it asks by inserting a component and reads the answer
/// from its own agent view once a game system has written it.
///
/// It has to be an action rather than a leaf returning `Running`. A leaf is
/// re-entered on every resume, so it would re-insert the request every frame
/// and thrash the entity between archetypes; `start` runs once per invocation,
/// which is what "ask" means.
struct AskForCover;

impl BtAction<Blackboard<'_, '_, '_, '_, '_, Fighter>> for AskForCover {
    type State = ();

    fn start(&self, bb: &mut Blackboard<'_, '_, '_, '_, '_, Fighter>, _: ()) -> Option<()> {
        bb.agent_commands().insert(WantsCover);
        Some(())
    }

    fn is_in_progress(&self, _: &(), bb: &Blackboard<'_, '_, '_, '_, '_, Fighter>, _: ()) -> bool {
        bb.cover.is_none()
    }

    fn tick(&self, _: &mut (), _: &mut Blackboard<'_, '_, '_, '_, '_, Fighter>, _: ()) {}

    fn complete(&self, _: &mut (), _: &mut Blackboard<'_, '_, '_, '_, '_, Fighter>, _: ()) -> bool {
        true
    }
}

fn take_cover() -> impl BehaviorNode<Fighter> {
    seq((
        // Nothing to walk to yet: ask, and wait for the answer.
        select((
            check(|bb: &Blackboard<Fighter>| bb.cover.is_some()),
            action(AskForCover),
        )),
        leaf(|bb: &mut Blackboard<Fighter>| {
            let Some(spot) = bb.cover.map(|c| c.0) else {
                return NodeResult::Failure;
            };
            if position(bb).distance(spot) < 8.0 {
                return NodeResult::Success;
            }
            step_towards(bb, spot);
            NodeResult::Running
        }),
    ))
}

// --- the three minds ---------------------------------------------------------

/// Runs at the player and swings.
pub fn chaser() -> impl BehaviorNode<Fighter> {
    select((
        seq((
            check(|bb: &Blackboard<Fighter>| range_to_player(bb) < 24.0),
            leaf(|bb: &mut Blackboard<Fighter>| {
                bb.health.0 -= 0.05;
                NodeResult::Success
            }),
        )),
        leaf(|bb: &mut Blackboard<Fighter>| {
            let player = bb.shared.player;
            step_towards(bb, player);
            NodeResult::Success
        }),
    ))
}

/// Keeps its distance and fires, reloading when dry.
pub fn sniper() -> impl BehaviorNode<Fighter> {
    select((
        reload(),
        seq((
            check(|bb: &Blackboard<Fighter>| range_to_player(bb) < 180.0),
            leaf(|bb: &mut Blackboard<Fighter>| {
                let away = position(bb) * 2.0 - bb.shared.player;
                step_towards(bb, away);
                NodeResult::Success
            }),
        )),
        seq((
            check(|bb: &Blackboard<Fighter>| range_to_player(bb) < 420.0),
            check(has_ammo),
            leaf(shoot),
        )),
        leaf(|bb: &mut Blackboard<Fighter>| {
            let player = bb.shared.player;
            step_towards(bb, player);
            NodeResult::Success
        }),
    ))
}

/// Fights until hurt, then goes to ground. Chooses by state, so revalidation
/// is what moves it between the two.
pub fn coward() -> impl BehaviorNode<Fighter> {
    choose!(|bb: &Blackboard<Fighter>| match hurt(bb) {
        true => take_cover(),
        false => chaser(),
    })
}
