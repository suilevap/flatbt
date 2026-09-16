use core::ops::{Deref, DerefMut};
use core::time::Duration;

use bevy_ecs::prelude::*;
use bevy_ecs::query::{IterQueryData, QueryData};
use bevy_ecs::system::{ReadOnlySystemParam, SystemParamItem};
use bevy_ecs::world::CommandQueue;
use flatbt_core::EntryMode;

/// The agent view a context reads from and writes back to.
pub type AgentItem<'w, 's, C> = <<C as BehaviorContext>::Agent as QueryData>::Item<'w, 's>;

/// The shared view a context reads from.
pub type ParamItem<'w, 's, C> = SystemParamItem<'w, 's, <C as BehaviorContext>::Param>;

/// Declares what one family of behavior trees sees, and how it is gathered.
///
/// The tree does not touch the ECS. It reads and writes [`Snapshot`], a plain
/// struct this trait fills before the tick and writes back after it. That is
/// what keeps a tree a value rather than a borrow: no lifetimes in any node
/// signature, nothing tying an invocation to the update that started it, and a
/// tree that can be exercised in a unit test with no [`World`] at all.
///
/// [`Agent`] and [`Param`] declare the access [`read`] and [`write`] may use.
/// Bevy schedules the tick against other systems from that declaration, and
/// [`IterQueryData`] keeps agent access disjoint between entities, which is what
/// makes [`BehaviorPlugin::parallel`] safe.
///
/// [`Snapshot`]: BehaviorContext::Snapshot
/// [`Agent`]: BehaviorContext::Agent
/// [`Param`]: BehaviorContext::Param
/// [`read`]: BehaviorContext::read
/// [`write`]: BehaviorContext::write
/// [`BehaviorPlugin::parallel`]: crate::BehaviorPlugin::parallel
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Component, PartialEq)]
/// struct Ammo(u32);
///
/// /// What the tree sees. Plain data, no borrows.
/// struct Guard {
///     ammo: u32,
/// }
///
/// /// What `read` and `write` may touch.
/// #[derive(QueryData)]
/// #[query_data(mutable)]
/// struct GuardAccess {
///     ammo: &'static mut Ammo,
/// }
///
/// impl BehaviorContext for Guard {
///     type Agent = GuardAccess;
///     type Param = ();
///     type Snapshot = Self;
///
///     fn read(_: Entity, agent: &GuardAccessItem, _: &()) -> Guard {
///         Guard { ammo: agent.ammo.0 }
///     }
///
///     fn write(guard: &Guard, agent: &mut GuardAccessItem) {
///         agent.ammo.set_if_neq(Ammo(guard.ammo));
///     }
/// }
/// ```
pub trait BehaviorContext: Send + Sync + 'static {
    /// Per-agent component access, fetched from the entity that owns the
    /// behavior. An entity whose components do not match is skipped.
    type Agent: IterQueryData + 'static;

    /// Read-only world access shared by every agent: resources, lookup queries.
    /// Use `()` when nothing beyond the agent is needed.
    type Param: ReadOnlySystemParam + 'static;

    /// What the tree reads and writes: a plain struct, carrying whatever the
    /// tree needs, including copies of shared values worth taking per agent.
    type Snapshot: Send + Sync + 'static;

    /// Gathers the snapshot before the tick.
    ///
    /// Everything the tree needs has to be decided here, which is also the
    /// cheapest place to decide it: work done once per agent per tick beats the
    /// same work done in several nodes, and anything derived costs less to carry
    /// than what it was derived from.
    fn read(
        entity: Entity,
        agent: &AgentItem<'_, '_, Self>,
        shared: &ParamItem<'_, '_, Self>,
    ) -> Self::Snapshot;

    /// Writes the snapshot back after the tick.
    ///
    /// Called every tick, so assign through [`Mut::set_if_neq`] or an explicit
    /// comparison where spurious change detection would cost something.
    ///
    /// [`Mut::set_if_neq`]: bevy_ecs::change_detection::DetectChangesMut::set_if_neq
    fn write(snapshot: &Self::Snapshot, agent: &mut AgentItem<'_, '_, Self>);

    /// Decides, per agent per tick, whether a suspended invocation continues or
    /// reconsiders from the root.
    ///
    /// Resuming is the cheap path and the default: a decision already taken
    /// stands, and an invocation that ends reconsiders on its own because the
    /// next one starts fresh. This is for abandoning a branch that has not
    /// ended -- a walk that takes a hundred frames while the reason for it
    /// expires. Deciding here rather than storing a mode per agent means the
    /// answer comes from the snapshot the tree already has.
    ///
    /// A fresh invocation always enters as [`EntryMode::Evaluate`].
    fn entry_mode(_bb: &Blackboard<Self>) -> EntryMode
    where
        Self: Sized,
    {
        EntryMode::Resume
    }
}

/// What every node of a [`Behavior<C>`](crate::Behavior) reads and writes: one
/// agent's snapshot, for one update, plus what it may defer to the world.
///
/// This is the `C` of [`flatbt_core::BtNode`] for Bevy trees. It owns its data,
/// so it has no lifetimes: nodes name it as `Blackboard<Guard>`, and an action
/// implements `BtAction<Blackboard<Guard>>` without spelling anything else.
///
/// [`Deref`] targets the snapshot, so `bb.ammo` reaches it while `bb.entity`
/// stays on the blackboard itself.
pub struct Blackboard<C: BehaviorContext> {
    /// The entity that owns the behavior.
    pub entity: Entity,
    /// What [`BehaviorContext::read`] gathered for this update.
    pub agent: C::Snapshot,
    /// Absent until a node defers something. Most agents never do, and an empty
    /// queue is not free: dropping one walks its buffer, which at one per agent
    /// per tick cost a fifth of the whole tick.
    queue: Option<CommandQueue>,
}

impl<C: BehaviorContext> Blackboard<C> {
    /// A blackboard over `snapshot`, as the tick system builds one.
    pub fn new(entity: Entity, snapshot: C::Snapshot) -> Self {
        Self {
            entity,
            agent: snapshot,
            queue: None,
        }
    }

    /// Defers a world edit beyond what the snapshot can carry: spawning,
    /// despawning, or touching another entity. Applied after the tick system.
    ///
    /// Edits to the agent's own components belong in the snapshot, which
    /// [`BehaviorContext::write`] puts back without going through the world.
    pub fn queue(&mut self, command: impl Command<Out = ()>) {
        self.queue.get_or_insert_default().push(command);
    }

    /// Commands targeting the agent entity.
    pub fn agent_commands(&mut self) -> EntityCommandQueue<'_> {
        EntityCommandQueue {
            entity: self.entity,
            queue: self.queue.get_or_insert_default(),
        }
    }

    /// Hands the deferred edits to the caller, if there were any.
    pub(crate) fn take_queue(&mut self) -> Option<CommandQueue> {
        self.queue.take()
    }
}

/// Deferred edits aimed at one entity. Returned by
/// [`Blackboard::agent_commands`].
pub struct EntityCommandQueue<'a> {
    entity: Entity,
    queue: &'a mut CommandQueue,
}

impl EntityCommandQueue<'_> {
    /// Inserts a bundle on the entity.
    pub fn insert(&mut self, bundle: impl Bundle) -> &mut Self {
        let entity = self.entity;
        self.queue.push(move |world: &mut World| {
            if let Ok(mut entity) = world.get_entity_mut(entity) {
                entity.insert(bundle);
            }
        });
        self
    }

    /// Removes a bundle from the entity.
    pub fn remove<B: Bundle>(&mut self) -> &mut Self {
        let entity = self.entity;
        self.queue.push(move |world: &mut World| {
            if let Ok(mut entity) = world.get_entity_mut(entity) {
                entity.remove::<B>();
            }
        });
        self
    }

    /// Despawns the entity.
    pub fn despawn(&mut self) {
        let entity = self.entity;
        self.queue.push(move |world: &mut World| {
            world.despawn(entity);
        });
    }
}

impl<C: BehaviorContext> Deref for Blackboard<C> {
    type Target = C::Snapshot;

    fn deref(&self) -> &Self::Target {
        &self.agent
    }
}

impl<C: BehaviorContext> DerefMut for Blackboard<C> {
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
