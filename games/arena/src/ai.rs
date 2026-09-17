//! Three enemy minds over one blackboard.
//!
//! The point of the crate: whether one [`BehaviorContext`] really serves a
//! family of trees, and what an action does when it needs to ask the world
//! something the context never declared.

use core::time::Duration;

use bevy::ecs::query::QueryData;
use bevy::prelude::*;
use flatbt::bevy::prelude::*;
use flatbt::prelude::{BtAction, BtNode, action, choose, scope};

use crate::world::{Ammo, Arena, CoverTarget, Health, Intent, Speed, WantsCover};

/// What every enemy tree sees and decides. Plain data: the tree never touches
/// the ECS, so nothing in a node signature carries a lifetime, and any of this
/// can be exercised in a unit test with no `World`.
///
/// Two halves that do not overlap. Everything above `intent` is what the world
/// said, written by `read` and only read by the tree. `intent` is what the tree
/// decided, written only by the tree and carried back out by `write`. No field
/// is both, which is the point: a tree that changed `ammo` directly would be
/// deciding what a shot costs, and that belongs to the weapon.
///
/// One snapshot serves all three trees. Shared values worth having per agent --
/// the player's position -- are copied in rather than borrowed, so they cost
/// eight bytes instead of a lifetime parameter.
pub struct Fighter {
    pub position: Vec2,
    pub health: f32,
    pub ammo: u32,
    pub speed: f32,
    /// Filled by `resolve_cover_requests`, a tick after a tree asks.
    pub cover: Option<Vec2>,
    pub player: Vec2,
    /// Whether this agent's turn to reconsider falls in this tick. Decided in
    /// `read`, where it costs one bool instead of carrying two `Duration`s.
    pub rethink: bool,
    /// What this tick's tree decided. Fresh every tick, because `read` builds
    /// it fresh: an intent is what the agent wants *now*, never a leftover.
    pub intent: Intent,
}

/// The access `read` and `write` may use. Declared once, for all three trees.
#[derive(QueryData)]
#[query_data(mutable)]
pub struct FighterAccess {
    transform: &'static Transform,
    health: &'static Health,
    ammo: &'static Ammo,
    speed: &'static Speed,
    cover: Option<&'static CoverTarget>,
    /// The only thing a tree writes. Everything else here is read-only, which
    /// the borrow checker now enforces rather than a convention.
    intent: &'static mut Intent,
}

/// How often a fighter is allowed to abandon what it is doing.
///
/// A tree reconsiders on its own whenever an invocation ends, so this is not
/// what makes it reactive. It is for the branch that does *not* end: walking to
/// cover takes a hundred frames, and a coward healed halfway there should turn
/// around. `evaluate_every` staggers the agents so the whole population does
/// not reconsider on one frame.
const RETHINK: Duration = Duration::from_millis(100);

impl BehaviorContext for Fighter {
    type Agent = FighterAccess;
    type Param = Res<'static, Arena>;
    type Snapshot = Self;

    fn read(entity: Entity, agent: &FighterAccessItem, arena: &Res<Arena>) -> Fighter {
        Fighter {
            position: agent.transform.translation.truncate(),
            health: agent.health.0,
            ammo: agent.ammo.0,
            speed: agent.speed.0,
            cover: agent.cover.map(|c| c.0),
            player: arena.player,
            rethink: evaluate_every(RETHINK, arena.elapsed, arena.delta, entity)
                == EntryMode::Evaluate,
            intent: Intent::default(),
        }
    }

    /// One field, and only when it changed: an agent that wants the same thing
    /// it wanted last tick does not dirty anything.
    fn write(fighter: &Fighter, agent: &mut FighterAccessItem) {
        agent.intent.set_if_neq(fighter.intent);
    }

    fn entry_mode(bb: &Blackboard<Fighter>) -> EntryMode {
        if bb.rethink {
            EntryMode::Evaluate
        } else {
            EntryMode::Resume
        }
    }
}

// --- shared pieces -----------------------------------------------------------

fn range_to_player(bb: &Blackboard<Fighter>) -> f32 {
    bb.position.distance(bb.player)
}

/// Asks to be somewhere. How fast, and whether anything is in the way, is
/// `apply_movement`'s business.
fn head_for(bb: &mut Blackboard<Fighter>, target: Vec2) {
    bb.intent.move_to = Some(target);
}

fn hurt(bb: &Blackboard<Fighter>) -> bool {
    bb.health < 40.0
}

fn has_ammo(bb: &Blackboard<Fighter>) -> bool {
    bb.ammo > 0
}

fn shoot(bb: &mut Blackboard<Fighter>) -> NodeResult {
    bb.intent.shoot = true;
    NodeResult::Success
}

/// Reloading takes several ticks, and holds its progress across them.
struct Reload {
    ticks: u32,
}

impl BtAction<Blackboard<Fighter>> for Reload {
    /// Ticks elapsed so far. Kept between updates; dropped when it ends.
    type State = u32;

    fn start(&self, _: &mut Blackboard<Fighter>, _: ()) -> Option<u32> {
        Some(0)
    }

    fn is_in_progress(&self, elapsed: &u32, _: &Blackboard<Fighter>, _: ()) -> bool {
        *elapsed < self.ticks
    }

    fn tick(&self, elapsed: &mut u32, _: &mut Blackboard<Fighter>, _: ()) {
        *elapsed += 1;
    }

    fn complete(&self, _: &mut u32, bb: &mut Blackboard<Fighter>, _: ()) -> bool {
        bb.intent.reload = true;
        true
    }
}

fn reload() -> impl BehaviorNode<Fighter> {
    seq((
        check(|bb: &Blackboard<Fighter>| !has_ammo(bb)),
        action(Reload { ticks: 30 }),
    ))
}

/// Walks to a spot the tree was handed. Takes it as a parameter rather than
/// reading it back off the blackboard, so it cannot run without one.
struct WalkTo;

impl BtNode<Blackboard<Fighter>, &Vec2> for WalkTo {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        bb: &mut Blackboard<Fighter>,
        spot: &Vec2,
        _: EntryMode,
    ) -> NodeResult {
        if bb.position.distance(*spot) < 8.0 {
            return NodeResult::Success;
        }
        head_for(bb, *spot);
        NodeResult::Running
    }
}

/// The deferred query, feeding a scope local.
///
/// A tree cannot run an arbitrary query -- the context declares its access up
/// front -- so it asks by inserting a component and `resolve_cover_requests`
/// answers into another. `ask` waits for that answer and writes it into `spot`,
/// which is where the ECS and the scope meet: the question is answered by a
/// system, and the answer arrives as an ordinary local. `WalkTo` then reads a
/// `Vec2`, not an `Option<Vec2>` -- the branch cannot be entered without one.
///
/// The local is per invocation, so leaving this branch and coming back asks
/// again rather than walking to a spot chosen for an older situation.
fn take_cover() -> impl BehaviorNode<Fighter> {
    scope! {
        let spot: Vec2;
        sequence {
            ask(WantsCover, |bb: &Blackboard<Fighter>| bb.cover).with(out spot);
            WalkTo.with(spot);
        }
    }
}

// --- the three minds ---------------------------------------------------------

/// Runs at the player and swings.
pub fn chaser() -> impl BehaviorNode<Fighter> {
    select((
        seq((
            check(|bb: &Blackboard<Fighter>| range_to_player(bb) < 24.0),
            leaf(|bb: &mut Blackboard<Fighter>| {
                bb.intent.melee = true;
                NodeResult::Success
            }),
        )),
        leaf(|bb: &mut Blackboard<Fighter>| {
            let player = bb.player;
            head_for(bb, player);
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
                let away = bb.position * 2.0 - bb.player;
                head_for(bb, away);
                NodeResult::Success
            }),
        )),
        seq((
            check(|bb: &Blackboard<Fighter>| range_to_player(bb) < 420.0),
            check(has_ammo),
            leaf(shoot),
        )),
        leaf(|bb: &mut Blackboard<Fighter>| {
            let player = bb.player;
            head_for(bb, player);
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
