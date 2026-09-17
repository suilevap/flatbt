//! An optional policy for [`BehaviorPlugin::entry_mode`], not part of the
//! integration proper: it takes a clock and an entity and returns a mode.
//!
//! [`BehaviorPlugin::entry_mode`]: crate::BehaviorPlugin::entry_mode

use core::time::Duration;

use bevy_ecs::prelude::*;
use flatbt_core::EntryMode;

/// [`EntryMode::Evaluate`] on the one tick where this agent's slice of `period`
/// elapses, [`EntryMode::Resume`] on every other.
///
/// Nothing here is privileged: `entry_mode` returns a mode and this returns a
/// mode, so a game that wants a different policy writes its own and never
/// mentions this one. It is here because staggering is the answer most often
/// wanted, and because getting it wrong -- putting a whole population on one
/// frame -- is easy.
///
/// A population that reconsiders on a timer would otherwise do it on the same
/// frame and spike. Each agent's slot is derived from its [`Entity`], so the
/// work spreads across the period and nothing is stored per agent. Pass the
/// clock the snapshot already carries.
///
/// A tick longer than `period` still evaluates once, never twice.
pub fn evaluate_every(
    period: Duration,
    elapsed: Duration,
    delta: Duration,
    entity: Entity,
) -> EntryMode {
    let period = nanos(period);
    if period == 0 {
        return EntryMode::Evaluate;
    }
    let delta = nanos(delta);
    if delta >= period {
        return EntryMode::Evaluate;
    }
    // A multiplicative hash, so entities spawned together -- consecutive
    // indices -- land in different slots rather than sharing one.
    let hash = u64::from(entity.index_u32().wrapping_mul(2_654_435_761));
    let phase = ((u128::from(hash) * u128::from(period)) >> 32) as u64;
    // The slot boundary falls inside this tick exactly when the offset clock
    // has less than a tick left of its current period. One remainder rather
    // than the two divisions the quotients would take: this runs once per agent
    // per tick, and a constant period folds it into a multiply.
    if nanos(elapsed).saturating_add(phase) % period < delta {
        EntryMode::Evaluate
    } else {
        EntryMode::Resume
    }
}

/// Nanoseconds as [`u64`], which holds 584 years of them.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
