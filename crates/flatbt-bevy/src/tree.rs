use core::marker::PhantomData;

use bevy_ecs::prelude::*;
use flatbt_core::{BtNode, EntryMode, NodeResult};

/// A tree that can drive agents whose blackboard is `C`, with one state type.
///
/// What this adds over `BtNode<C>` is `State = Self::Data`, which pins the
/// invocation state to a single named type so [`Behavior`] has a size to
/// reserve. A bound would leave it a projection; an equality to a written-out
/// type cannot be spelled, because a composed tree's state is nested control
/// state over closures. An associated type is the only equality target left, so
/// it takes a trait -- and in return position it then names a subtree without
/// naming its type: `fn patrol() -> impl BehaviorNode<Guard>`.
///
/// Bevy components must be `Send + Sync`, so a tree and its inline state carry
/// that on top of FlatBT's own bounds.
pub trait BehaviorNode<C>: BtNode<C, State = Self::Data> + Send + Sync + 'static {
    /// Inline invocation state for the whole tree.
    type Data: Default + Send + Sync + 'static;
}

impl<C, N, S> BehaviorNode<C> for N
where
    N: BtNode<C, State = S> + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
{
    type Data = S;
}

/// Names one tree, and builds it once.
///
/// Implemented for every `Fn() -> impl BehaviorNode<C>`, so a plain function is
/// a tree's name. The builder is the identity: the tree type cannot be, because
/// two builders can return the same one with different node configuration, and
/// a closure's return type cannot be projected on stable in any case.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Component, Default)]
/// struct Guard {
///     ammo: u32,
/// }
///
/// fn advance(step: u32) -> impl BehaviorNode<Guard> {
///     leaf(move |guard: &mut Guard| {
///         guard.ammo += step;
///         NodeResult::Success
///     })
/// }
///
/// // Two names for the same tree type, each with its own configuration.
/// fn calm() -> impl BehaviorNode<Guard> {
///     advance(1)
/// }
///
/// fn angry() -> impl BehaviorNode<Guard> {
///     advance(10)
/// }
/// ```
pub trait TreeBuilder<C>: Send + Sync + 'static {
    /// The tree this builder produces.
    type Tree: BehaviorNode<C>;

    fn build(&self) -> Self::Tree;
}

impl<C, N, F> TreeBuilder<C> for F
where
    N: BehaviorNode<C>,
    F: Fn() -> N + Send + Sync + 'static,
{
    type Tree = N;

    fn build(&self) -> N {
        self()
    }
}

/// How a tick re-enters a suspended invocation, given the blackboard.
pub type EntryModeFn<C> = fn(&C) -> EntryMode;

/// The one tree named by `F`, built once and shared by every agent running it.
///
/// Inserted by [`BehaviorPlugin`](crate::BehaviorPlugin). Trees are immutable
/// definitions, so they belong in a resource rather than copied into each agent.
/// Public so a game ticking its agents itself reads the tree the same way the
/// generated system does; see [`Behavior::tick`].
#[derive(Resource)]
pub struct BehaviorTree<C: Send + Sync + 'static, F: TreeBuilder<C>> {
    tree: F::Tree,
    entry_mode: EntryModeFn<C>,
    // Load-bearing: it keeps `C` a direct field use. Reached only through the
    // `F::Tree` projection, `C` sends the monomorphization collector through
    // every blanket impl behind it and over the recursion limit.
    context: PhantomData<fn() -> C>,
}

impl<C: Send + Sync + 'static, F: TreeBuilder<C>> BehaviorTree<C, F> {
    /// Builds the tree once. [`BehaviorPlugin`](crate::BehaviorPlugin) does
    /// this; a game registering its own tick does it itself.
    pub fn new(builder: &F, entry_mode: EntryModeFn<C>) -> Self {
        Self {
            tree: builder.build(),
            entry_mode,
            context: PhantomData,
        }
    }

    /// The definition, to hand to [`Behavior::tick`].
    pub fn get(&self) -> &F::Tree {
        &self.tree
    }

    /// How this tree re-enters a suspended invocation.
    pub fn entry_mode(&self) -> EntryModeFn<C> {
        self.entry_mode
    }
}

/// One agent's invocation state for the tree named by `F`.
///
/// Holds only what is per-agent: the state of a suspended invocation, sized
/// exactly for that tree. The tree itself lives once in a resource, and the
/// builder is zero-sized when it is a plain function, so an agent costs exactly
/// its invocation state.
///
/// Neither type parameter is written out. Both come from the builder, which
/// names the tree here exactly as it does at registration:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Component, Default)]
/// struct Guard {
///     ammo: u32,
///     fire: bool,
/// }
///
/// fn shoot() -> impl BehaviorNode<Guard> {
///     seq((
///         check(|guard: &Guard| guard.ammo > 0),
///         leaf(|guard: &mut Guard| {
///             guard.fire = true;
///             NodeResult::Success
///         }),
///     ))
/// }
///
/// # let mut world = World::new();
/// # let mut commands = world.commands();
/// commands.spawn((Guard { ammo: 3, fire: false }, Behavior::for_tree(shoot)));
/// ```
///
/// Agents running different trees are different component types, so they sit in
/// different archetypes and are ticked by their own system. To write the type
/// out, name the builder as a function pointer:
/// `Behavior::for_tree(shoot as fn() -> _)`.
#[derive(Component)]
pub struct Behavior<C: Send + Sync + 'static, F: TreeBuilder<C>> {
    state: Option<<F::Tree as BehaviorNode<C>>::Data>,
    // Only the builder's type is needed; the value it was named by is not kept,
    // so it cannot be mistaken for per-agent configuration. Load-bearing beyond
    // that: it keeps `C` and `F` direct field uses, as above.
    builder: PhantomData<fn() -> (C, F)>,
}

impl<C: Send + Sync + 'static, F: TreeBuilder<C>> Behavior<C, F> {
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

    /// Runs one update against a tree fetched by the caller.
    ///
    /// This is the seam under [`BehaviorPlugin`](crate::BehaviorPlugin).
    /// Everything above it -- the query, the schedule, whether agents run in
    /// parallel -- is a system the plugin writes, and a game can write instead.
    /// This part it cannot: a composed tree's state type cannot be named, so
    /// only a generic over the builder can hold it, and that is what the library
    /// is for.
    pub fn tick(&mut self, tree: &F::Tree, bb: &mut C, mode: EntryMode) {
        // A fresh invocation always enters as Evaluate, whatever the caller asks
        // for, and a terminal result drops invocation state. Same as FlatBT's
        // own root lifetime in `update`.
        let mode = if self.state.is_none() {
            EntryMode::Evaluate
        } else {
            mode
        };
        if self.run(tree, bb, mode) == NodeResult::Failure && mode == EntryMode::Resume {
            // The continuation is gone, and a resumed update never consulted
            // anything above it, so the failure says nothing about what the tree
            // would choose now. The next update would enter as Evaluate anyway
            // -- this only spares the agent a tick of doing nothing.
            let _ = self.run(tree, bb, EntryMode::Evaluate);
        }
    }

    fn run(&mut self, tree: &F::Tree, bb: &mut C, mode: EntryMode) -> NodeResult {
        let result = tree.update(
            self.state.get_or_insert_with(Default::default),
            bb,
            (),
            mode,
        );
        if result != NodeResult::Running {
            self.state = None;
        }
        result
    }
}

impl<C: Send + Sync + 'static, F: TreeBuilder<C>> core::fmt::Debug for Behavior<C, F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Behavior")
            .field("running", &self.state.is_some())
            .finish()
    }
}
