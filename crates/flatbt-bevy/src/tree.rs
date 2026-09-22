use core::any::TypeId;
use core::marker::PhantomData;
use core::time::Duration;

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::{DeferredWorld, EntityWorldMut};
use flatbt_core::{BtNode, EntryMode, NodeResult, update_slot};

/// A tree that can drive agents whose blackboard is `C` and whose decisions are
/// `A`, with one state type.
///
/// What this adds over `BtNode<C, A>` is `State = Self::Data`, which pins the
/// invocation state to a single named type so [`Behavior`] has a size to
/// reserve. A bound would leave it a projection; an equality to a written-out
/// type cannot be spelled, because a composed tree's state is nested control
/// state over closures. An associated type is the only equality target left, so
/// it takes a trait -- and in return position it then names a subtree without
/// naming its type: `fn patrol() -> impl BehaviorNode<Guard, Act>`.
///
/// Bevy components must be `Send + Sync`, so a tree and its inline state carry
/// that on top of FlatBT's own bounds.
pub trait BehaviorNode<C, A = ()>:
    BtNode<C, A, State = Self::Data> + Send + Sync + 'static
{
    /// Inline invocation state for the whole tree.
    type Data: Default + Send + Sync + 'static;
}

impl<C, A, N, S> BehaviorNode<C, A> for N
where
    N: BtNode<C, A, State = S> + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
{
    type Data = S;
}

/// Names one tree, and builds it once.
///
/// Implemented for every `Fn() -> impl BehaviorNode<C, A>`, so a plain function
/// is a tree's name. The builder is the identity: the tree type cannot be,
/// because two builders can return the same one with different node
/// configuration, and a closure's return type cannot be projected on stable in
/// any case.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Component, Default)]
/// struct Guard {
///     ammo: u32,
/// }
///
/// #[derive(Component, Clone, Copy, PartialEq)]
/// enum Act {
///     Firing,
/// }
///
/// fn shoot(rounds: u32) -> impl BehaviorNode<Guard, Act> {
///     leaf(move |guard: &mut Guard| {
///         if guard.ammo >= rounds {
///             NodeResult::Running(Act::Firing)
///         } else {
///             NodeResult::Failure
///         }
///     })
/// }
///
/// // Two names for the same tree type, each with its own configuration.
/// fn careful() -> impl BehaviorNode<Guard, Act> {
///     shoot(3)
/// }
///
/// fn reckless() -> impl BehaviorNode<Guard, Act> {
///     shoot(1)
/// }
/// ```
pub trait TreeBuilder<C, A = ()>: Send + Sync + 'static {
    /// The tree this builder produces.
    type Tree: BehaviorNode<C, A>;

    fn build(&self) -> Self::Tree;
}

impl<C, A, N, F> TreeBuilder<C, A> for F
where
    N: BehaviorNode<C, A>,
    F: Fn() -> N + Send + Sync + 'static,
{
    type Tree = N;

    fn build(&self) -> N {
        self()
    }
}

/// What a tick does with one agent, decided from its blackboard.
///
/// **[`Evaluate`](Tick::Evaluate) every tick is always correct.** The other two
/// are optimisations, and a tree that breaks under one of them is a tree with a
/// bug: `Resume` costs an agent the chance to change its mind, and `Skip` costs
/// it the tick entirely, so both make an agent less responsive and neither
/// makes it behave differently once it does run.
///
/// [`Resume`](Tick::Resume) continues down the path a suspended invocation
/// chose, so a `select` does not rescan and a `choose!` does not re-pick. It is
/// what lets a standing action restate its act -- following a moving target
/// without re-deciding whether to follow it.
///
/// [`Skip`](Tick::Skip) does not enter the tree at all and leaves both the
/// suspended invocation and the agent's current act exactly as they were. It is
/// the only one of the three that no node inside the tree can approximate: a
/// node can continue an invocation or end it, but not leave it untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Do not enter the tree. The suspended invocation and the standing act are
    /// left exactly as they were.
    Skip,
    /// Enter, continuing where the invocation left off.
    Resume,
    /// Enter, reconsidering from the root.
    Evaluate,
}

impl Tick {
    /// The entry mode, or `None` when the agent is not entered at all.
    pub fn entry_mode(self) -> Option<EntryMode> {
        match self {
            Tick::Skip => None,
            Tick::Resume => Some(EntryMode::Resume),
            Tick::Evaluate => Some(EntryMode::Evaluate),
        }
    }
}

/// Which agent a tick is for, and when it falls.
///
/// The clock is Bevy's `Time`, which inside `FixedUpdate` is the fixed clock.
/// Both durations are zero when the app has no `Time` resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickAt {
    pub entity: Entity,
    /// Time elapsed since startup.
    pub elapsed: Duration,
    /// Time since the previous tick of this schedule.
    pub delta: Duration,
}

/// What a tick does with one agent, given its blackboard and when it falls.
pub type TickFn<C> = fn(&C, TickAt) -> Tick;

/// The one tree named by `F`, built once and shared by every agent running it.
///
/// Inserted by [`BehaviorPlugin`](crate::BehaviorPlugin). Trees are immutable
/// definitions, so they belong in a resource rather than copied into each agent.
/// Public so a game ticking its agents itself reads the tree the same way the
/// generated system does; see [`Behavior::tick`].
#[derive(Resource)]
pub struct BehaviorTree<C: Send + Sync + 'static, A: Send + Sync + 'static, F: TreeBuilder<C, A>> {
    tree: F::Tree,
    tick_mode: TickFn<C>,
    // Load-bearing: it keeps `C` and `A` direct field uses. Reached only through
    // the `F::Tree` projection, they send the monomorphization collector through
    // every blanket impl behind them and over the recursion limit.
    context: PhantomData<fn() -> (C, A)>,
}

impl<C: Send + Sync + 'static, A: Send + Sync + 'static, F: TreeBuilder<C, A>>
    BehaviorTree<C, A, F>
{
    /// Builds the tree once. [`BehaviorPlugin`](crate::BehaviorPlugin) does
    /// this; a game registering its own tick does it itself.
    pub fn new(builder: &F, tick_mode: TickFn<C>) -> Self {
        Self {
            tree: builder.build(),
            tick_mode,
            context: PhantomData,
        }
    }

    /// The definition, to hand to [`Behavior::tick`].
    pub fn get(&self) -> &F::Tree {
        &self.tree
    }

    /// What this tree's tick does with one agent.
    pub fn tick_mode(&self) -> TickFn<C> {
        self.tick_mode
    }
}

/// One agent's invocation state for the tree named by `F`.
///
/// Holds only what is per-agent: the state of a suspended invocation, sized
/// exactly for that tree. The tree itself lives once in a resource, and the
/// builder is zero-sized when it is a plain function, so an agent costs exactly
/// its invocation state.
///
/// Neither the blackboard nor the act type is written out. Both come from the
/// builder, which names the tree here exactly as it does at registration:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Component, Default)]
/// struct Guard {
///     ammo: u32,
/// }
///
/// #[derive(Component, Clone, Copy, PartialEq)]
/// enum Act {
///     Firing,
/// }
///
/// fn shoot() -> impl BehaviorNode<Guard, Act> {
///     leaf(|guard: &mut Guard| {
///         if guard.ammo > 0 {
///             NodeResult::Running(Act::Firing)
///         } else {
///             NodeResult::Failure
///         }
///     })
/// }
///
/// # let mut world = World::new();
/// # let mut commands = world.commands();
/// commands.spawn((Guard { ammo: 3 }, Behavior::for_tree(shoot)));
/// ```
///
/// Agents running different trees are different component types, so they sit in
/// different archetypes and are ticked by their own system. To write the type
/// out, name the builder as a function pointer:
/// `Behavior::for_tree(shoot as fn() -> _)`.
///
/// # Starting, stopping, changing
///
/// This component is what makes an entity an agent, so it is also the switch:
///
/// - **insert** it (with the blackboard) and the tree starts on the next tick;
/// - **remove** it -- or despawn the entity -- and the agent stops. Its
///   standing act is taken back as the component goes -- a removal hook does
///   it, so it does not wait for a tick that may never come -- and nothing
///   keeps carrying out the last order the tree gave.
/// - **replace** it with a `Behavior` for another tree and the same happens:
///   the old act is released, and the new tree decides from scratch.
/// - [`restart`](Self::restart) drops the suspended invocation without stopping
///   the agent, so the next tick enters from the root.
///
/// An entity carrying this without a blackboard is ticked by nothing, and
/// keeps whatever act it last had: stopping an agent is
/// [`stop_behavior`](BehaviorCommands::stop_behavior), not taking its
/// blackboard away.
#[derive(Component)]
#[component(on_remove = release_act::<A>)]
pub struct Behavior<C: Send + Sync + 'static, A: Send + Sync + 'static, F: TreeBuilder<C, A>> {
    state: Option<<F::Tree as BehaviorNode<C, A>>::Data>,
    // Only the builder's type is needed; the value it was named by is not kept,
    // so it cannot be mistaken for per-agent configuration. Load-bearing beyond
    // that: it keeps `C`, `A` and `F` direct field uses, as above.
    builder: Names<C, A, F>,
}

/// The type parameters an agent carries without storing anything for them.
type Names<C, A, F> = PhantomData<fn() -> (C, A, F)>;

impl<C: Send + Sync + 'static, A: Send + Sync + 'static, F: TreeBuilder<C, A>> Behavior<C, A, F> {
    /// Runs the tree named by `builder`, restarted after every terminal result.
    ///
    /// `builder` is not called here: it names the tree, which
    /// [`BehaviorPlugin::for_tree`](crate::BehaviorPlugin::for_tree) built when
    /// the app was built.
    ///
    /// Only the builder's *type* selects the tree, and the value is not kept:
    /// the tree was built once at registration. A builder that captures
    /// configuration therefore configures nothing here. Vary a tree by writing a
    /// second builder function, not by capturing different values in one
    /// closure.
    pub fn for_tree(_builder: F) -> Self {
        Self {
            state: None,
            builder: PhantomData,
        }
    }

    /// Drops the suspended invocation, so the next tick enters from the root.
    ///
    /// The agent keeps running this tree -- this only forgets where it was, for
    /// a game that changes an agent's situation so thoroughly that continuing
    /// the current action would be wrong. The act is left to the next tick,
    /// which decides it again and removes it if the fresh invocation decides
    /// nothing. To stop the agent instead of restarting it, remove the
    /// component.
    pub fn restart(&mut self) {
        self.state = None;
    }

    /// Runs one update and hands back what the agent is now doing.
    ///
    /// `None` means the invocation ended, so the agent is doing nothing: the
    /// tick system removes its act component. This is the seam under
    /// [`BehaviorPlugin`](crate::BehaviorPlugin) -- everything above it (the
    /// query, the schedule, whether agents run in parallel) is a system the
    /// plugin writes and a game can write instead. This part it cannot: a
    /// composed tree's state type cannot be named, so only a generic over the
    /// builder can hold it.
    pub fn tick(&mut self, tree: &F::Tree, bb: &mut C, mode: EntryMode) -> Option<A> {
        // A fresh invocation enters as Evaluate whatever the caller asks for.
        let resumed = self.state.is_some() && mode == EntryMode::Resume;
        match update_slot(tree, &mut self.state, bb, mode) {
            NodeResult::Running(act) => Some(act),
            // The continuation is gone, and a resumed update never consulted
            // anything above it, so the failure says nothing about what the tree
            // would choose now. The next update would enter as Evaluate anyway
            // -- this only spares the agent a tick of doing nothing.
            NodeResult::Failure if resumed => {
                update_slot(tree, &mut self.state, bb, EntryMode::Evaluate).act()
            }
            _ => None,
        }
    }
}

impl<C: Send + Sync + 'static, A: Send + Sync + 'static, F: TreeBuilder<C, A>> core::fmt::Debug
    for Behavior<C, A, F>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Behavior")
            .field("running", &self.state.is_some())
            .finish()
    }
}

/// Takes back an agent's standing act when it stops running its tree.
///
/// Registered as [`Behavior`]'s removal hook, so it runs the moment the
/// component goes -- by `remove`, by `despawn`, or by being swapped for another
/// tree -- whatever schedule that happened in, and whether or not the tick
/// system ever runs again. The tick cannot do this itself: an agent that
/// stopped is no longer in its query, and an act component outlives the tick
/// that wrote it by design, since the systems carrying it out run afterwards.
/// Without this, removing a `Behavior` would leave the last order standing and
/// the world would go on obeying it.
///
/// `A` need not be a component at all -- a tree that decides nothing has no act
/// to release -- so the act type is looked up rather than named.
fn release_act<A: Send + Sync + 'static>(mut world: DeferredWorld, ctx: HookContext) {
    let Some(act) = world.components().get_id(TypeId::of::<A>()) else {
        return;
    };
    let entity = ctx.entity;
    if !world
        .get_entity(entity)
        .is_ok_and(|agent| agent.contains_id(act))
    {
        return;
    }
    world.commands().queue(move |world: &mut World| {
        // The entity is gone when the `Behavior` went with it in a despawn.
        if let Ok(mut agent) = world.get_entity_mut(entity) {
            agent.remove_by_id(act);
        }
    });
}

/// Stopping and restarting an agent, by naming its tree.
///
/// A `Behavior`'s type cannot be written down -- a builder's return type is
/// opaque, so `remove::<Behavior<Guard, Act, _>>()` is not something a game can
/// spell. These name the tree the way everything else does, by its builder, and
/// are implemented for both [`EntityCommands`] and [`EntityWorldMut`]:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Component, Default)]
/// # struct Guard { ammo: u32 }
/// # #[derive(Component, Clone, Copy, PartialEq)]
/// # enum Act { Firing }
/// # fn shoot() -> impl BehaviorNode<Guard, Act> {
/// #     leaf(|_: &mut Guard| NodeResult::Running(Act::Firing))
/// # }
/// fn disarm(mut commands: Commands, downed: Query<Entity, With<Act>>) {
///     for agent in downed.iter() {
///         // The tree stops and the standing act goes with it.
///         commands.entity(agent).stop_behavior(shoot);
///     }
/// }
/// ```
///
/// Starting an agent needs nothing new: insert `Behavior::for_tree(shoot)`.
pub trait BehaviorCommands {
    /// Stops this agent running the tree named by `tree`, releasing its act.
    ///
    /// Removing the component is all this does -- the release is
    /// [`Behavior`]'s own removal hook, so it happens however the component
    /// goes.
    fn stop_behavior<C, A, F>(&mut self, tree: F) -> &mut Self
    where
        C: Send + Sync + 'static,
        A: Send + Sync + 'static,
        F: TreeBuilder<C, A>;

    /// Drops this agent's suspended invocation, so its next tick enters the
    /// tree named by `tree` from the root. See [`Behavior::restart`].
    fn restart_behavior<C, A, F>(&mut self, tree: F) -> &mut Self
    where
        C: Send + Sync + 'static,
        A: Send + Sync + 'static,
        F: TreeBuilder<C, A>;
}

impl BehaviorCommands for EntityCommands<'_> {
    fn stop_behavior<C, A, F>(&mut self, _tree: F) -> &mut Self
    where
        C: Send + Sync + 'static,
        A: Send + Sync + 'static,
        F: TreeBuilder<C, A>,
    {
        self.try_remove::<Behavior<C, A, F>>()
    }

    fn restart_behavior<C, A, F>(&mut self, tree: F) -> &mut Self
    where
        C: Send + Sync + 'static,
        A: Send + Sync + 'static,
        F: TreeBuilder<C, A>,
    {
        self.queue(move |mut agent: EntityWorldMut| {
            agent.restart_behavior::<C, A, F>(tree);
        })
    }
}

impl BehaviorCommands for EntityWorldMut<'_> {
    fn stop_behavior<C, A, F>(&mut self, _tree: F) -> &mut Self
    where
        C: Send + Sync + 'static,
        A: Send + Sync + 'static,
        F: TreeBuilder<C, A>,
    {
        self.remove::<Behavior<C, A, F>>()
    }

    fn restart_behavior<C, A, F>(&mut self, _tree: F) -> &mut Self
    where
        C: Send + Sync + 'static,
        A: Send + Sync + 'static,
        F: TreeBuilder<C, A>,
    {
        if let Some(mut behavior) = self.get_mut::<Behavior<C, A, F>>() {
            behavior.restart();
        }
        self
    }
}
