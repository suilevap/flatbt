use core::marker::PhantomData;

use bevy_ecs::prelude::*;
use flatbt_core::{BtNode, EntryMode, NodeResult};

use crate::plugin::request_registration;
use crate::{BehaviorContext, Bt};

/// A tree that can drive agents of context `C`.
///
/// Implemented for every [`BtNode`] whose context is [`Bt<C>`](Bt) at any update
/// lifetime, so it is the one bound worth naming: `Bt<C>` carries the update's
/// borrows, which makes every use of it higher-ranked over five lifetimes.
/// Naming it also names a subtree without spelling out its type:
/// `fn patrol() -> impl BehaviorNode<Guard>`.
///
/// Bevy resources and components must be `Sync`, so a tree and its inline state
/// carry that requirement on top of FlatBT's own bounds.
pub trait BehaviorNode<C: BehaviorContext>:
    for<'w, 's, 'q, 'a, 'c> BtNode<Bt<'w, 's, 'q, 'a, 'c, C>, State = Self::Data>
    + Send
    + Sync
    + 'static
{
    /// Inline invocation state for the whole tree.
    type Data: Default + Send + Sync + 'static;
}

impl<C, N, S> BehaviorNode<C> for N
where
    C: BehaviorContext,
    N: for<'w, 's, 'q, 'a, 'c> BtNode<Bt<'w, 's, 'q, 'a, 'c, C>, State = S> + Send + Sync + 'static,
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
/// # #[derive(Component)]
/// # struct Ammo(u32);
/// # #[derive(QueryData)]
/// # #[query_data(mutable)]
/// # struct Guard { ammo: &'static mut Ammo }
/// # impl BehaviorContext for Guard { type Agent = Self; type Param = (); }
/// fn advance(step: u32) -> impl BehaviorNode<Guard> {
///     leaf(move |bt: &mut Bt<Guard>| {
///         bt.ammo.0 += step;
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
/// Inserted by [`FlatBtPlugin`](crate::FlatBtPlugin) or
/// [`BehaviorPlugin`](crate::BehaviorPlugin). Trees are immutable definitions,
/// so they belong in a resource rather than copied into each agent.
#[derive(Resource)]
pub struct BehaviorTree<C: BehaviorContext, F: TreeBuilder<C>> {
    tree: F::Tree,
    // Load-bearing: it keeps `C` a direct field use. Reached only through the
    // `F::Tree` projection, `C` sends the monomorphization collector through
    // every blanket impl behind it and over the recursion limit.
    context: PhantomData<fn() -> C>,
}

impl<C: BehaviorContext, F: TreeBuilder<C>> BehaviorTree<C, F> {
    pub fn new(tree: F::Tree) -> Self {
        Self {
            tree,
            context: PhantomData,
        }
    }

    pub fn get(&self) -> &F::Tree {
        &self.tree
    }
}

/// One agent's invocation state for the tree named by `F`.
///
/// Holds only what is per-agent: the state of a suspended invocation, sized
/// exactly for that tree, and how to re-enter it. The tree itself lives once in
/// [`BehaviorTree<C, F>`](BehaviorTree), and the builder is zero-sized when it
/// is a plain function, so an agent costs its invocation state and two flags.
///
/// Neither type parameter is written out. Both come from the builder, which
/// names the tree here exactly as it does at registration:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Component)]
/// # struct Ammo(u32);
/// # #[derive(QueryData)]
/// # #[query_data(mutable)]
/// # struct Guard { ammo: &'static mut Ammo }
/// # impl BehaviorContext for Guard { type Agent = Self; type Param = (); }
/// fn shoot() -> impl BehaviorNode<Guard> {
///     seq((
///         check(|bt: &Bt<Guard>| bt.ammo.0 > 0),
///         leaf(|bt: &mut Bt<Guard>| {
///             bt.ammo.0 -= 1;
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
#[component(on_add = request_registration::<C, F>)]
pub struct Behavior<C: BehaviorContext, F: TreeBuilder<C>> {
    state: Option<<F::Tree as BehaviorNode<C>>::Data>,
    builder: F,
    // See `BehaviorTree`: keeping `C` a direct field use, not a projection.
    context: PhantomData<fn() -> C>,
}

impl<C: BehaviorContext, F: TreeBuilder<C>> Behavior<C, F> {
    /// Runs the tree named by `builder`, restarted after every terminal result.
    /// Insert [`BehaviorPaused`] to stop it, [`BehaviorRevalidate`] to make the
    /// next tick reconsider its decisions.
    ///
    /// `builder` is not called here: it names the tree. The first agent to name
    /// a tree builds it, so no registration is needed beyond
    /// [`FlatBtPlugin`](crate::FlatBtPlugin).
    pub fn for_tree(builder: F) -> Self {
        Self {
            state: None,
            builder,
            context: PhantomData,
        }
    }

    /// Runs one update.
    ///
    /// `mode` is the caller's per-tick decision, not a stored setting: the tick
    /// systems resume unless the agent carries [`BehaviorRevalidate`].
    pub fn tick(
        &mut self,
        tree: &F::Tree,
        bt: &mut Bt<'_, '_, '_, '_, '_, C>,
        mode: EntryMode,
    ) -> NodeResult {
        // A fresh invocation always enters as Evaluate, whatever the caller asks
        // for, and a terminal result drops invocation state. Same as FlatBT's
        // own root lifetime in `update`.
        let mode = if self.state.is_none() {
            EntryMode::Evaluate
        } else {
            mode
        };
        let result = tree.update(
            self.state.get_or_insert_with(Default::default),
            bt,
            (),
            mode,
        );
        if result != NodeResult::Running {
            self.state = None;
        }
        result
    }

    pub(crate) fn builder(&self) -> &F {
        &self.builder
    }
}

impl<C: BehaviorContext, F: TreeBuilder<C>> core::fmt::Debug for Behavior<C, F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Behavior")
            .field("running", &self.state.is_some())
            .finish_non_exhaustive()
    }
}

/// Makes the next tick re-enter with [`EntryMode::Evaluate`], then is consumed.
///
/// Trees resume by default, which is the cheap path: decisions already taken
/// stand until something says otherwise. Revalidation is a tool the game aims —
/// on a timer, on a perception event, when an order changes — rather than a
/// per-agent setting, so it is a component any system can insert without naming
/// `Behavior<C, F>`.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct BehaviorRevalidate;

/// Stops every behavior on an entity until it is removed.
///
/// A tree can stop itself with [`Bt::pause`], which a flag on the component
/// could not offer: a node reaches the agent through [`Bt<C>`](Bt), which does
/// not name the behavior component. As a component it is also queryable and
/// removable from ordinary systems, which a flag was not.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct BehaviorPaused;
