//! The arena itself: what fighters are made of, and the world they read.
//!
//! Everything here is the game's truth. A tree never writes any of it: it
//! writes what it *wants* into its blackboard, and [`ai::carry_out`] decides
//! what that costs.
//!
//! [`ai::carry_out`]: crate::ai::carry_out

use core::time::Duration;

use bevy::prelude::*;

pub const ARENA: f32 = 900.0;

#[derive(Component, Debug, PartialEq)]
pub struct Health(pub f32);

#[derive(Component, Debug, PartialEq)]
pub struct Ammo(pub u32);

#[derive(Component, Debug, PartialEq)]
pub struct Speed(pub f32);

/// Static geometry a coward can hide behind.
#[derive(Component)]
pub struct Cover;

#[derive(Component)]
pub struct Player;

/// The shared world the gather reads.
///
/// One resource rather than several, so a system that needs something new reads
/// a new field instead of widening its access.
#[derive(Resource, Default, Debug)]
pub struct Arena {
    pub player: Vec2,
    /// The clock, so agents can pace their own revalidation.
    pub elapsed: Duration,
    pub delta: Duration,
    /// Counted here, so a gather system can run at its own rate.
    pub tick: u32,
}

pub fn track_arena(
    player: Query<&Transform, With<Player>>,
    time: Res<Time>,
    mut arena: ResMut<Arena>,
) {
    arena.tick += 1;
    arena.elapsed = time.elapsed();
    arena.delta = time.delta();
    if let Ok(transform) = player.single() {
        arena.player = transform.translation.truncate();
    }
}
