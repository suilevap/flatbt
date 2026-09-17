use core::ops::{Deref, DerefMut};

use bevy_ecs::message::Messages;
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

    /// Publishes what the tree decided, after the tick.
    ///
    /// Not a mirror of [`read`](BehaviorContext::read). A snapshot is better
    /// off when no field is both gathered and written: what the world said is
    /// the tree's input, and what the tree decided is an *intent* an ordinary
    /// system carries out. A tree that subtracts the round it fired is deciding
    /// what a shot costs, which belongs to the weapon; one that writes its own
    /// position is deciding how fast the agent is and what a frame is worth. Let
    /// it say `shoot` and `move_to` instead, and put the rest in `Agent` as `&`
    /// rather than `&mut` so the split is the borrow checker's business.
    ///
    /// Only runs when a node took `&mut` to the snapshot, so a tree that merely
    /// looked leaves change detection alone. Where a value can be written
    /// unchanged, [`Mut::set_if_neq`] keeps the rest of the engine out of it.
    ///
    /// Defaults to publishing nothing, which is right for a tree whose whole
    /// effect is deferred -- a message, a spawn, a component on another entity.
    /// Such a context wants no `&mut` in [`Agent`](BehaviorContext::Agent)
    /// either, and so no `#[query_data(mutable)]`.
    ///
    /// [`Mut::set_if_neq`]: bevy_ecs::change_detection::DetectChangesMut::set_if_neq
    fn write(_snapshot: &Self::Snapshot, _agent: &mut AgentItem<'_, '_, Self>) {}

    /// Decides, per agent per tick, whether a suspended invocation reconsiders
    /// from the root or continues where it left off.
    ///
    /// Reconsidering is the default, because it is the answer that cannot be
    /// wrong: a tree that only ever resumes never leaves the branch it is in,
    /// so `select` never rescans and `choose!` never re-picks. Resuming is an
    /// optimisation, correct exactly when the standing decision is known to
    /// still hold -- a suspended branch nothing in the world has invalidated.
    ///
    /// It is worth measuring before reaching for. Over 100 000 agents on three
    /// trees the arena cannot tell the two apart, because a tree whose
    /// invocations end each tick has nothing to resume into; the saving is in
    /// long-running branches, which is also where resuming is most likely to be
    /// wrong.
    ///
    /// [`evaluate_every`](crate::evaluate_every) is the usual middle: resume
    /// between an agent's slots, reconsider on the one tick its slot comes up.
    ///
    /// A fresh invocation always enters as [`EntryMode::Evaluate`] whatever this
    /// returns.
    fn entry_mode(_bb: &Blackboard<Self>) -> EntryMode
    where
        Self: Sized,
    {
        EntryMode::Evaluate
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
/// stays on the blackboard itself. Reading goes through [`Deref`] and writing
/// through [`DerefMut`], which is what lets the tick skip
/// [`BehaviorContext::write`] entirely for an agent whose tree only looked.
pub struct Blackboard<C: BehaviorContext> {
    /// The entity that owns the behavior.
    pub entity: Entity,
    /// What [`BehaviorContext::read`] gathered for this update. Private so that
    /// reaching it goes through [`Deref`], which is what notices a write.
    snapshot: C::Snapshot,
    /// Set by [`DerefMut`], so [`BehaviorContext::write`] is skipped for an
    /// agent whose tree only looked. Same bargain as Bevy's own change
    /// detection: taking `&mut` counts, whether or not anything changed.
    written: bool,
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
            snapshot,
            written: false,
            queue: None,
        }
    }

    /// The snapshot as the tree left it.
    pub fn snapshot(&self) -> &C::Snapshot {
        &self.snapshot
    }

    /// Takes the snapshot away, for a caller driving a tree without a tick
    /// system -- a unit test, say.
    pub fn into_snapshot(self) -> C::Snapshot {
        self.snapshot
    }

    /// Whether anything took `&mut` to the snapshot this update. `false` means
    /// [`BehaviorContext::write`] has nothing to put back.
    pub fn written(&self) -> bool {
        self.written
    }

    /// Defers a world edit beyond what the snapshot can carry: spawning,
    /// despawning, or touching another entity. Applied after the tick system.
    ///
    /// Edits to the agent's own components belong in the snapshot, which
    /// [`BehaviorContext::write`] puts back without going through the world.
    pub fn queue(&mut self, command: impl Command<Out = ()>) {
        self.queue.get_or_insert_default().push(command);
    }

    /// Writes a Bevy message, applied after the tick like any deferred edit.
    ///
    /// The channel for "this happened" -- a shot fired, a target lost -- where
    /// a marker component would be the wrong shape. A marker has to be cleared
    /// by someone, and inserting and removing one moves the entity between
    /// archetypes twice a tick, which at a large population costs more than
    /// everything the tree did.
    pub fn write_message<M: Message>(&mut self, message: M) {
        self.queue(move |world: &mut World| {
            if let Some(mut messages) = world.get_resource_mut::<Messages<M>>() {
                messages.write(message);
            }
        });
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
        &self.snapshot
    }
}

impl<C: BehaviorContext> DerefMut for Blackboard<C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.written = true;
        &mut self.snapshot
    }
}
