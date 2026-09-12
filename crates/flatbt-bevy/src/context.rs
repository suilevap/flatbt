use core::ops::{Deref, DerefMut};
use core::time::Duration;

use bevy_ecs::prelude::*;
use bevy_ecs::query::{IterQueryData, QueryData};
use bevy_ecs::system::{ReadOnlySystemParam, SystemParamItem};
use flatbt_core::EntryMode;

/// Declares the world access one family of behavior trees needs.
///
/// The implementing type is a marker: it names the context, and every
/// [`Behavior<Self>`](crate::Behavior) on an entity is driven by the same system.
/// Access is declared once here instead of per node, so Bevy can schedule the
/// tick against other systems and iterate agents in parallel.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::BehaviorContext;
/// #[derive(Component)]
/// struct Ammo(u32);
///
/// #[derive(QueryData)]
/// #[query_data(mutable)]
/// struct Guard {
///     ammo: &'static mut Ammo,
/// }
///
/// impl BehaviorContext for Guard {
///     type Agent = Self;
///     type Param = ();
/// }
/// ```
pub trait BehaviorContext: Send + Sync + 'static {
    /// Per-agent component access, fetched from the entity that owns the behavior.
    ///
    /// [`IterQueryData`] restricts this to access that is disjoint between
    /// entities, which is what makes mutable agent state safe to tick in parallel.
    /// An entity whose components do not match is skipped.
    type Agent: IterQueryData + 'static;

    /// Read-only world access shared by every agent: resources, lookup queries.
    ///
    /// Use `()` when the tree only touches its own entity. Mutations beyond the
    /// agent's own components go through [`Bt::commands`], which defers them to
    /// the end of the schedule step.
    type Param: ReadOnlySystemParam + 'static;

    /// Decides, per agent per tick, whether a suspended invocation continues or
    /// reconsiders from the root.
    ///
    /// Resuming is the cheap path and the default: a decision already taken
    /// stands. [`EntryMode::Evaluate`] re-runs the choices above the active node,
    /// which is what makes a tree react. Deciding here rather than storing a mode
    /// per agent means the answer comes from the world the tree already declared
    /// — a timer, a changed resource, a perception component — and costs nothing
    /// when it is a constant.
    ///
    /// A fresh invocation always enters as [`EntryMode::Evaluate`], whatever this
    /// returns.
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// # use bevy_ecs::query::QueryData;
    /// # use flatbt_bevy::prelude::*;
    /// # #[derive(Component)]
    /// # struct Ammo(u32);
    /// # #[derive(Resource)]
    /// # struct Alarm(bool);
    /// # #[derive(QueryData)]
    /// # #[query_data(mutable)]
    /// # struct Guard { ammo: &'static mut Ammo }
    /// impl BehaviorContext for Guard {
    ///     type Agent = Self;
    ///     type Param = Res<'static, Alarm>;
    ///
    ///     fn entry_mode(bt: &Bt<Guard>) -> EntryMode {
    ///         if bt.shared.is_changed() {
    ///             EntryMode::Evaluate
    ///         } else {
    ///             EntryMode::Resume
    ///         }
    ///     }
    /// }
    /// ```
    fn entry_mode(_bt: &Bt<'_, '_, '_, '_, '_, Self>) -> EntryMode
    where
        Self: Sized,
    {
        EntryMode::Resume
    }
}

/// The agent view for a context, as nodes receive it.
pub type AgentItem<'w, 's, C> = <<C as BehaviorContext>::Agent as QueryData>::Item<'w, 's>;

/// The shared view for a context, as nodes receive it.
pub type ParamItem<'w, 's, C> = SystemParamItem<'w, 's, <C as BehaviorContext>::Param>;

/// Context passed to every node of a [`Behavior<C>`](crate::Behavior).
///
/// This is the `C` of [`flatbt_core::BtNode`] for Bevy trees. It borrows world
/// data for the duration of one update, so node state can never retain it.
///
/// [`Deref`] targets the agent view, so `bt.health` reaches the agent's
/// component while `bt.entity`, `bt.shared`, and `bt.commands` stay available on
/// the context itself.
///
/// The lifetimes are an implementation detail of the tick system. Write
/// `Bt<Guard>` and let them be inferred.
pub struct Bt<'w, 's, 'q, 'a, 'c, C: BehaviorContext> {
    /// The entity that owns the behavior.
    pub entity: Entity,
    /// Per-agent component access declared by [`BehaviorContext::Agent`].
    pub agent: AgentItem<'a, 'q, C>,
    /// Shared read-only access declared by [`BehaviorContext::Param`].
    pub shared: &'a ParamItem<'w, 's, C>,
    /// Deferred world mutation. Applied after the tick system finishes.
    pub commands: Commands<'c, 'c>,
}

impl<C: BehaviorContext> Bt<'_, '_, '_, '_, '_, C> {
    /// Commands targeting the agent entity.
    pub fn agent_commands(&mut self) -> EntityCommands<'_> {
        self.commands.entity(self.entity)
    }

    /// Stops every behavior on this agent, from the next tick, by inserting
    /// [`BehaviorPaused`](crate::BehaviorPaused). Remove it to resume.
    pub fn pause(&mut self) {
        self.agent_commands().insert(crate::BehaviorPaused);
    }
}

impl<'q, 'a, C: BehaviorContext> Deref for Bt<'_, '_, 'q, 'a, '_, C> {
    type Target = AgentItem<'a, 'q, C>;

    fn deref(&self) -> &Self::Target {
        &self.agent
    }
}

impl<C: BehaviorContext> DerefMut for Bt<'_, '_, '_, '_, '_, C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.agent
    }
}

/// [`EntryMode::Evaluate`] on the one tick where this agent's slice of `period`
/// elapses, [`EntryMode::Resume`] on every other.
///
/// A population that reconsiders on a timer would otherwise do it on the same
/// frame and spike. Each agent's slot is derived from its [`Entity`], so the
/// work spreads across the period and nothing is stored per agent. Pass the
/// clock the context already reads:
///
/// ```
/// # use core::time::Duration;
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Component)]
/// # struct Ammo(u32);
/// # #[derive(Resource)]
/// # struct Clock { elapsed: Duration, delta: Duration }
/// # #[derive(QueryData)]
/// # #[query_data(mutable)]
/// # struct Guard { ammo: &'static mut Ammo }
/// impl BehaviorContext for Guard {
///     type Agent = Self;
///     type Param = Res<'static, Clock>;
///
///     fn entry_mode(bt: &Bt<Guard>) -> EntryMode {
///         evaluate_every(
///             Duration::from_millis(200),
///             bt.shared.elapsed,
///             bt.shared.delta,
///             bt.entity,
///         )
///     }
/// }
/// ```
///
/// A tick longer than `period` still evaluates once, never twice.
pub fn evaluate_every(
    period: Duration,
    elapsed: Duration,
    delta: Duration,
    entity: Entity,
) -> EntryMode {
    let period = period.as_nanos();
    if period == 0 {
        return EntryMode::Evaluate;
    }
    // A multiplicative hash, so entities spawned together -- consecutive
    // indices -- land in different slots rather than sharing one.
    let phase = (u128::from(entity.index_u32().wrapping_mul(2_654_435_761)) * period) >> 32;
    let now = elapsed.as_nanos() + phase;
    let previous = elapsed.saturating_sub(delta).as_nanos() + phase;
    if now / period == previous / period {
        EntryMode::Resume
    } else {
        EntryMode::Evaluate
    }
}
