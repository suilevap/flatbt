use core::marker::PhantomData;

use bevy_ecs::prelude::*;
use flatbt_core::{BtNode, EntryMode, NodeResult};

#[cfg(debug_assertions)]
use crate::plugin::warn_unregistered;
use crate::{BehaviorContext, Blackboard};

/// How a tick re-enters a suspended invocation, as a tree can override it.
pub type EntryModeFn<C> = fn(&Blackboard<C>) -> EntryMode;

/// A tree that can drive agents of context `C`, with one state type.
///
/// What this adds over `BtNode<Blackboard<C>>` is `State = Self::Data`, which
/// pins the invocation state to a single named type so [`Behavior`] has a size
/// to reserve. A bound would leave it a projection; an equality to a written-out
/// type cannot be spelled, because a composed tree's state is nested control
/// state over closures. An associated type is the only equality target left, so
/// it takes a trait -- and in return position it then names a subtree without
/// naming its type: `fn patrol() -> impl BehaviorNode<Guard>`.
///
/// Bevy resources and components must be `Sync`, so a tree and its inline state
/// carry that requirement on top of FlatBT's own bounds.
pub trait BehaviorNode<C: BehaviorContext>:
    BtNode<Blackboard<C>, State = Self::Data> + Send + Sync + 'static
{
    /// Inline invocation state for the whole tree.
    type Data: Default + Send + Sync + 'static;
}

impl<C, N, S> BehaviorNode<C> for N
where
    C: BehaviorContext,
    N: BtNode<Blackboard<C>, State = S> + Send + Sync + 'static,
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
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Component, PartialEq)]
/// # struct Ammo(u32);
/// # #[derive(QueryData)]
/// # #[query_data(mutable)]
/// # struct GuardAccess { ammo: &'static mut Ammo }
/// # struct Guard { ammo: u32 }
/// # impl BehaviorContext for Guard {
/// #     type Agent = GuardAccess;
/// #     type Param = ();
/// #     type Snapshot = Self;
/// #     fn read(_: Entity, a: &GuardAccessItem, _: &()) -> Guard { Guard { ammo: a.ammo.0 } }
/// #     fn write(g: &Guard, a: &mut GuardAccessItem) { a.ammo.set_if_neq(Ammo(g.ammo)); }
/// # }
/// fn advance(step: u32) -> impl BehaviorNode<Guard> {
///     leaf(move |bb: &mut Blackboard<Guard>| {
///         bb.ammo += step;
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
pub trait TreeBuilder<C: BehaviorContext>: Send + Sync + 'static {
    /// The tree this builder produces.
    type Tree: BehaviorNode<C>;

    fn build(&self) -> Self::Tree;
}

impl<C, N, F> TreeBuilder<C> for F
where
    C: BehaviorContext,
    N: BehaviorNode<C>,
    F: Fn() -> N + Send + Sync + 'static,
{
    type Tree = N;

    fn build(&self) -> N {
        self()
    }
}

/// The one tree named by `F`, built once and shared by every agent running it.
///
/// Inserted by [`BehaviorPlugin`](crate::BehaviorPlugin). Trees are immutable
/// definitions, so they belong in a resource rather than copied into each agent.
#[derive(Resource)]
pub(crate) struct BehaviorTree<C: BehaviorContext, F: TreeBuilder<C>> {
    tree: F::Tree,
    entry_mode: EntryModeFn<C>,
    // Load-bearing: it keeps `C` a direct field use. Reached only through the
    // `F::Tree` projection, `C` sends the monomorphization collector through
    // every blanket impl behind it and over the recursion limit.
    context: PhantomData<fn() -> C>,
}

impl<C: BehaviorContext, F: TreeBuilder<C>> BehaviorTree<C, F> {
    pub(crate) fn new(tree: F::Tree, entry_mode: Option<EntryModeFn<C>>) -> Self {
        Self {
            tree,
            entry_mode: entry_mode.unwrap_or(C::entry_mode),
            context: PhantomData,
        }
    }

    pub(crate) fn get(&self) -> &F::Tree {
        &self.tree
    }

    pub(crate) fn entry_mode(&self) -> EntryModeFn<C> {
        self.entry_mode
    }
}

/// One agent's invocation state for the tree named by `F`.
///
/// Holds only what is per-agent: the state of a suspended invocation, sized
/// exactly for that tree, and how to re-enter it. The tree itself lives once in
/// a resource of its own, and the builder is zero-sized when it is a plain
/// function, so an agent costs exactly its invocation state.
///
/// Neither type parameter is written out. Both come from the builder, which
/// names the tree here exactly as it does at registration:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Component, PartialEq)]
/// # struct Ammo(u32);
/// # #[derive(QueryData)]
/// # #[query_data(mutable)]
/// # struct GuardAccess { ammo: &'static mut Ammo }
/// # struct Guard { ammo: u32 }
/// # impl BehaviorContext for Guard {
/// #     type Agent = GuardAccess;
/// #     type Param = ();
/// #     type Snapshot = Self;
/// #     fn read(_: Entity, a: &GuardAccessItem, _: &()) -> Guard { Guard { ammo: a.ammo.0 } }
/// #     fn write(g: &Guard, a: &mut GuardAccessItem) { a.ammo.set_if_neq(Ammo(g.ammo)); }
/// # }
/// fn shoot() -> impl BehaviorNode<Guard> {
///     seq((
///         check(|bb: &Blackboard<Guard>| bb.ammo > 0),
///         leaf(|bb: &mut Blackboard<Guard>| {
///             bb.ammo -= 1;
///             NodeResult::Success
///         }),
///     ))
/// }
///
/// # let mut world = World::new();
/// # let mut commands = world.commands();
/// commands.spawn((Ammo(3), Behavior::for_tree(shoot)));
/// ```
///
/// Agents running different trees are different component types, so they sit in
/// different archetypes and are ticked by their own system. To write the type
/// out, name the builder as a function pointer:
/// `Behavior::for_tree(shoot as fn() -> _)`.
#[derive(Component)]
// A spawn-time hook to catch a tree nobody registered. It is a development
// convenience, not a guarantee -- a component means nothing without a system,
// here as anywhere in Bevy -- so it costs nothing in a release build.
#[cfg_attr(debug_assertions, component(on_add = warn_unregistered::<C, F>))]
pub struct Behavior<C: BehaviorContext, F: TreeBuilder<C>> {
    state: Option<<F::Tree as BehaviorNode<C>>::Data>,
    // Only the builder's type is needed; the value it was named by is not kept,
    // so it cannot be mistaken for per-agent configuration. Load-bearing beyond
    // that: it keeps `C` and `F` direct field uses. Reached only through the
    // `F::Tree` projection, `C` sends the monomorphization collector through
    // every blanket impl behind it and over the recursion limit.
    builder: PhantomData<fn() -> (C, F)>,
}

impl<C: BehaviorContext, F: TreeBuilder<C>> Behavior<C, F> {
    /// Runs the tree named by `builder`, restarted after every terminal result.
    /// Whether a suspended invocation resumes or reconsiders is
    /// [`BehaviorContext::entry_mode`](crate::BehaviorContext::entry_mode).
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

    /// Runs one update. `mode` comes from
    /// [`BehaviorContext::entry_mode`](crate::BehaviorContext::entry_mode).
    ///
    /// The result is not reported anywhere: at the root it says only that this
    /// invocation ended, and the next tick starts a new one. A tree that has
    /// something to say says it through `bb`.
    pub(crate) fn tick(&mut self, tree: &F::Tree, bb: &mut Blackboard<C>, mode: EntryMode) {
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

    fn run(&mut self, tree: &F::Tree, bb: &mut Blackboard<C>, mode: EntryMode) -> NodeResult {
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

impl<C: BehaviorContext, F: TreeBuilder<C>> core::fmt::Debug for Behavior<C, F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Behavior")
            .field("running", &self.state.is_some())
            .finish_non_exhaustive()
    }
}
