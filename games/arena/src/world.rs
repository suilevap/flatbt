//! The arena itself: what fighters are made of, and the world they read.

use core::time::Duration;

use bevy::prelude::*;

pub const ARENA: f32 = 900.0;

#[derive(Component, Debug)]
pub struct Health(pub f32);

#[derive(Component, Debug)]
pub struct Ammo(pub u32);

#[derive(Component, Debug)]
pub struct Speed(pub f32);

/// Static geometry a coward can hide behind.
#[derive(Component)]
pub struct Cover;

/// Asked for by a tree, answered by [`resolve_cover_requests`] on a later tick.
#[derive(Component)]
pub struct WantsCover;

/// The answer. Absent until the request is served.
#[derive(Component, Debug)]
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
