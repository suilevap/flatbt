//! The arena itself: what fighters are made of, and the world they read.

use core::time::Duration;

use bevy::prelude::*;

pub const ARENA: f32 = 900.0;

#[derive(Component, Debug, PartialEq)]
pub struct Health(pub f32);

#[derive(Component, Debug, PartialEq)]
pub struct Ammo(pub u32);

#[derive(Component, Debug, PartialEq)]
pub struct Speed(pub f32);

/// What a tree decided this tick. The other half of the blackboard: `read`
/// never touches these, and nothing but a tree writes them.
///
/// Splitting it this way is what keeps the AI out of the game's bookkeeping. A
/// tree that wanted to shoot used to subtract the round itself, which put the
/// rules of the weapon inside the behaviour; now it says `shoot` and
/// [`fire_and_reload`] owns what that costs. The tree can be wrong about
/// whether it *should* shoot. It cannot be wrong about how much ammunition a
/// shot takes, because it never knew.
#[derive(Component, Clone, Copy, Default, PartialEq, Debug)]
pub struct Intent {
    /// Where the agent wants to be. Absent means standing still.
    pub move_to: Option<Vec2>,
    pub shoot: bool,
    pub melee: bool,
    pub reload: bool,
}

/// Static geometry a coward can hide behind.
#[derive(Component)]
pub struct Cover;

/// Asked for by a tree, answered by [`resolve_cover_requests`] on a later tick.
#[derive(Component, Clone)]
pub struct WantsCover;

/// The answer. Absent until the request is served.
#[derive(Component, Debug, PartialEq)]
pub struct CoverTarget(pub Vec2);

#[derive(Component)]
pub struct Player;

/// Shared read-only world the trees see.
///
/// One resource rather than several: the context declares it once, and a tree
/// that needs something new reads a new field instead of widening its access.
#[derive(Resource, Default, Debug)]
pub struct Arena {
    pub player: Vec2,
    /// The clock, so trees can pace their own revalidation.
    pub elapsed: Duration,
    pub delta: Duration,
}

pub fn track_arena(
    player: Query<&Transform, With<Player>>,
    time: Res<Time>,
    mut arena: ResMut<Arena>,
) {
    arena.elapsed = time.elapsed();
    arena.delta = time.delta();
    if let Ok(transform) = player.single() {
        arena.player = transform.translation.truncate();
    }
}

/// Moves an agent towards what its tree asked for, at the speed the agent has.
///
/// The tree never touches [`Transform`]: it does not know how fast this agent
/// is, whether something blocks the way, or what a frame is worth.
/// Spread across the task pool: the access is disjoint per entity, same as the
/// tick's, and at this population a serial sweep costs more than the trees did.
pub fn apply_movement(mut agents: Query<(&Intent, &Speed, &mut Transform)>) {
    agents
        .par_iter_mut()
        .for_each(|(intent, speed, mut transform)| {
            let Some(target) = intent.move_to else {
                return;
            };
            let from = transform.translation.truncate();
            let step = (target - from).normalize_or_zero() * speed.0;
            if step != Vec2::ZERO {
                transform.translation += step.extend(0.0);
            }
        });
}

/// Spends and refills ammunition, and charges for a swing.
///
/// Every number here is the weapon's, not the tree's.
pub fn fire_and_reload(mut agents: Query<(&Intent, &mut Ammo, &mut Health)>) {
    agents
        .par_iter_mut()
        .for_each(|(intent, mut ammo, mut health)| {
            if intent.shoot && ammo.0 > 0 {
                ammo.0 -= 1;
            }
            if intent.reload {
                ammo.set_if_neq(Ammo(6));
            }
            if intent.melee {
                health.0 -= 0.05;
            }
        });
}

/// The deferred query a tree cannot make for itself.
///
/// A node asks by inserting [`WantsCover`]; this answers into [`CoverTarget`],
/// which the agent reads on a later tick. The tree never touches the cover
/// query, and the work is one system the game schedules where it likes.
pub fn resolve_cover_requests(
    asking: Query<(Entity, &Transform), With<WantsCover>>,
    cover: Query<&Transform, With<Cover>>,
    mut commands: Commands,
) {
    if asking.is_empty() {
        return;
    }
    let spots: Vec<Vec2> = cover.iter().map(|t| t.translation.truncate()).collect();
    for (entity, transform) in asking.iter() {
        let from = transform.translation.truncate();
        let nearest = spots.iter().copied().min_by(|a, b| {
            a.distance_squared(from)
                .total_cmp(&b.distance_squared(from))
        });
        let mut agent = commands.entity(entity);
        agent.remove::<WantsCover>();
        if let Some(spot) = nearest {
            agent.insert(CoverTarget(spot));
        }
    }
}
