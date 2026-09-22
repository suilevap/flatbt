//! Optional policies for [`BehaviorPlugin::tick_mode`], not part of the
//! integration proper: they take a clock and an entity and return a [`Tick`].
//!
//! [`BehaviorPlugin::tick_mode`]: crate::BehaviorPlugin::tick_mode

use core::time::Duration;

use bevy_ecs::prelude::*;

use crate::Tick;

/// [`Tick::Evaluate`] on the one tick where this agent's slice of `period`
/// elapses, [`Tick::Resume`] on every other.
///
/// Nothing here is privileged: `tick_mode` returns a `Tick` and so does this, so
/// a game wanting a different policy writes its own and never mentions this one.
/// It is here because staggering is the answer most often wanted, and because
/// getting it wrong -- putting a whole population on one frame -- is easy.
///
/// Each agent's slot is derived from its [`Entity`], so the work spreads across
/// the period and nothing is stored per agent. A tick longer than `period`
/// still evaluates once, never twice.
///
/// Every tick between slots still *resumes*, so an action in progress keeps
/// being ticked. Use [`act_every`] instead when there is nothing to tick.
pub fn evaluate_every(
    period: Duration,
    elapsed: Duration,
    delta: Duration,
    entity: Entity,
) -> Tick {
    stagger(period, elapsed, delta, entity, Tick::Resume)
}

/// [`Tick::Evaluate`] on this agent's slot, [`Tick::Skip`] on every other tick.
///
/// For a tree whose work happens entirely outside it -- every act it reports is
/// carried out by systems matching that component, and no node needs a tick of
/// its own to make progress. Then there is nothing to resume into between
/// slots, the standing act is left in place, and skipping is free.
///
/// Wrong for a tree with an action that advances on its own, since skipping
/// stops it advancing. If unsure, use [`evaluate_every`]: resuming does the
/// same thing and costs a tick.
pub fn act_every(period: Duration, elapsed: Duration, delta: Duration, entity: Entity) -> Tick {
    stagger(period, elapsed, delta, entity, Tick::Skip)
}

fn stagger(
    period: Duration,
    elapsed: Duration,
    delta: Duration,
    entity: Entity,
    between: Tick,
) -> Tick {
    let period = nanos(period);
    if period == 0 {
        return Tick::Evaluate;
    }
    let delta = nanos(delta);
    if delta >= period {
        return Tick::Evaluate;
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
        Tick::Evaluate
    } else {
        between
    }
}

/// Nanoseconds as [`u64`], which holds 584 years of them.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
