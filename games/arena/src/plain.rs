//! The same three minds with the blackboard as an ordinary component.
//!
//! No `BehaviorContext`, no `read`, no `write`. The gather is three systems
//! running at three different rates, which is the case the single `read`
//! function could not express: cover has to be searched for, so it is looked up
//! once every ten frames and only for agents that asked.

use bevy::prelude::*;
use flatbt::bevy::{Mind, Runs};
use flatbt::prelude::*;

use crate::world::{Ammo, Arena, Cover, Health, Speed};

/// Everything a tree reads and decides, in one component.
///
/// Filled by the systems below, read and written by the trees, carried out by
/// the systems after. Nothing in the library knows what is in it.
#[derive(Component, Default)]
pub struct Fighter {
    pub position: Vec2,
    pub health: f32,
    pub ammo: u32,
    pub speed: f32,
    pub player: Vec2,
    /// Answered by `find_cover`, which runs at a tenth of the tick rate.
    pub cover: Option<Vec2>,
    pub rethink: bool,
    // --- what the tree decided -------------------------------------------
    pub move_to: Option<Vec2>,
    pub shoot: bool,
    pub melee: bool,
    pub reload: bool,
    pub wants_cover: bool,
}

/// Every tick: the cheap half, straight off the agent's own components.
pub fn gather_agent(
    mut agents: Query<(&Transform, &Health, &Ammo, &Speed, &mut Fighter)>,
    arena: Res<Arena>,
) {
    agents
        .par_iter_mut()
        .for_each(|(transform, health, ammo, speed, mut fighter)| {
            let fighter = fighter.bypass_change_detection();
            fighter.position = transform.translation.truncate();
            fighter.health = health.0;
            fighter.ammo = ammo.0;
            fighter.speed = speed.0;
            fighter.player = arena.player;
            // Last tick's decisions are not this tick's.
            fighter.move_to = None;
            fighter.shoot = false;
            fighter.melee = false;
            fighter.reload = false;
        });
}

/// Every tenth tick, and only for agents that asked: the expensive half.
///
/// A stand-in for a raycast or a path query -- the thing a per-agent `read`
/// could not do, because it has neither a place to cache nor a rate of its own.
pub fn find_cover(
    mut agents: Query<(&Transform, &mut Fighter)>,
    cover: Query<&Transform, With<Cover>>,
    arena: Res<Arena>,
    mut next: Local<u32>,
) {
    if arena.tick < *next {
        return;
    }
    *next = arena.tick + 10;
    let spots: Vec<Vec2> = cover.iter().map(|t| t.translation.truncate()).collect();
    for (transform, mut fighter) in agents.iter_mut() {
        if !fighter.wants_cover {
            continue;
        }
        let from = transform.translation.truncate();
        let fighter = fighter.bypass_change_detection();
        fighter.cover = spots.iter().copied().min_by(|a, b| {
            a.distance_squared(from)
                .total_cmp(&b.distance_squared(from))
        });
        fighter.wants_cover = false;
    }
}

/// And the pace, which is neither of the above.
pub fn gather_pace(mut agents: Query<(Entity, &mut Fighter)>, arena: Res<Arena>) {
    agents.par_iter_mut().for_each(|(entity, mut fighter)| {
        fighter.bypass_change_detection().rethink =
            flatbt::bevy::evaluate_every(crate::ai::RETHINK, arena.elapsed, arena.delta, entity)
                == EntryMode::Evaluate;
    });
}

// --- the three minds, over a plain struct ------------------------------------

fn range_to_player(f: &Fighter) -> f32 {
    f.position.distance(f.player)
}

pub fn chaser() -> impl Mind<Fighter> {
    select((
        seq((
            check(|f: &Fighter| range_to_player(f) < 24.0),
            leaf(|f: &mut Fighter| {
                f.melee = true;
                NodeResult::Success
            }),
        )),
        leaf(|f: &mut Fighter| {
            f.move_to = Some(f.player);
            NodeResult::Success
        }),
    ))
}

struct Reload {
    ticks: u32,
}

impl BtAction<Fighter> for Reload {
    type State = u32;

    fn start(&self, _: &mut Fighter, _: ()) -> Option<u32> {
        Some(0)
    }

    fn is_in_progress(&self, elapsed: &u32, _: &Fighter, _: ()) -> bool {
        *elapsed < self.ticks
    }

    fn tick(&self, elapsed: &mut u32, _: &mut Fighter, _: ()) {
        *elapsed += 1;
    }

    fn complete(&self, _: &mut u32, f: &mut Fighter, _: ()) -> bool {
        f.reload = true;
        true
    }
}

pub fn sniper() -> impl Mind<Fighter> {
    select((
        seq((
            check(|f: &Fighter| f.ammo == 0),
            action(Reload { ticks: 30 }),
        )),
        seq((
            check(|f: &Fighter| range_to_player(f) < 180.0),
            leaf(|f: &mut Fighter| {
                f.move_to = Some(f.position * 2.0 - f.player);
                NodeResult::Success
            }),
        )),
        seq((
            check(|f: &Fighter| range_to_player(f) < 420.0 && f.ammo > 0),
            leaf(|f: &mut Fighter| {
                f.shoot = true;
                NodeResult::Success
            }),
        )),
        leaf(|f: &mut Fighter| {
            f.move_to = Some(f.player);
            NodeResult::Success
        }),
    ))
}

/// Asking is a field now, not a component insert: `find_cover` answers it.
fn take_cover() -> impl Mind<Fighter> {
    select((
        seq((
            check(|f: &Fighter| f.cover.is_some()),
            leaf(|f: &mut Fighter| {
                let Some(spot) = f.cover else {
                    return NodeResult::Failure;
                };
                if f.position.distance(spot) < 8.0 {
                    return NodeResult::Success;
                }
                f.move_to = Some(spot);
                NodeResult::Running
            }),
        )),
        leaf(|f: &mut Fighter| {
            f.wants_cover = true;
            NodeResult::Running
        }),
    ))
}

pub fn coward() -> impl Mind<Fighter> {
    choose!(|f: &Fighter| match f.health < 40.0 {
        true => take_cover(),
        false => chaser(),
    })
}

/// What the trees decided, carried out.
pub fn carry_out(mut agents: Query<(&Fighter, &mut Transform, &mut Ammo, &mut Health)>) {
    agents
        .par_iter_mut()
        .for_each(|(fighter, mut transform, mut ammo, mut health)| {
            if let Some(target) = fighter.move_to {
                let from = transform.translation.truncate();
                let step = (target - from).normalize_or_zero() * fighter.speed;
                if step != Vec2::ZERO {
                    transform.translation += step.extend(0.0);
                }
            }
            if fighter.shoot && ammo.0 > 0 {
                ammo.0 -= 1;
            }
            if fighter.reload {
                ammo.set_if_neq(Ammo(6));
            }
            if fighter.melee {
                health.0 -= 0.05;
            }
        });
}

pub fn spawn_for(index: u32) -> impl Bundle {
    let _ = index;
    Fighter::default()
}

pub type Agent<F> = Runs<Fighter, F>;
