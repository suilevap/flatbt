use core::marker::PhantomData;

use bevy_ecs::prelude::*;
use flatbt_core::{BtNode, EntryMode, NodeResult};

use crate::plugin::request_registration;
use crate::{BehaviorContext, Blackboard};

/// A tree that can drive agents of context `C`, with one state type.
///
/// `Blackboard<C>` carries the update's borrows, so being a node for it means being one
/// at every update: `for<'w, 's, 'q, 'a, 'c> BtNode<Blackboard<'w, 's, 'q, 'a, 'c, C>>`.
/// That much is only long to write. What this trait adds is `State = Self::Data`,
/// which pins the invocation state to a *single* type across that whole family,
/// and that cannot be written inline:
///
/// - `BtNode<..., State: Default + Send + Sync>` is a bound, not an equality, so
///   the state stays a separate projection per instantiation and
///   [`Behavior`] has no size to reserve.
/// - `BtNode<..., State = u32>` does pin it, but only to a type that can be
///   named. A composed tree's state is nested control state over closures, which
///   cannot be.
///
/// An associated type is the only equality target left, so it takes a trait. In
/// return position it then names a subtree without naming its type:
/// `fn patrol() -> impl BehaviorNode<Guard>`.
///
/// Bevy resources and components must be `Sync`, so a tree and its inline state
/// carry that requirement on top of FlatBT's own bounds.
pub trait BehaviorNode<C: BehaviorContext>:
    for<'w, 's, 'q, 'a, 'c> BtNode<Blackboard<'w, 's, 'q, 'a, 'c, C>, State = Self::Data>
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
    N: for<'w, 's, 'q, 'a, 'c> BtNode<Blackboard<'w, 's, 'q, 'a, 'c, C>, State = S>
        + Send
        + Sync
        + 'static,
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
///     leaf(move |bb: &mut Blackboard<Guard>| {
///         bb.ammo.0 += step;
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
pub(crate) struct BehaviorTree<C: BehaviorContext, F: TreeBuilder<C>> {
    tree: F::Tree,
    // Load-bearing: it keeps `C` a direct field use. Reached only through the
    // `F::Tree` projection, `C` sends the monomorphization collector through
    // every blanket impl behind it and over the recursion limit.
    context: PhantomData<fn() -> C>,
}

impl<C: BehaviorContext, F: TreeBuilder<C>> BehaviorTree<C, F> {
    pub(crate) fn new(tree: F::Tree) -> Self {
        Self {
            tree,
            context: PhantomData,
        }
    }

    pub(crate) fn get(&self) -> &F::Tree {
        &self.tree
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
/// # #[derive(Component)]
/// # struct Ammo(u32);
/// # #[derive(QueryData)]
/// # #[query_data(mutable)]
/// # struct Guard { ammo: &'static mut Ammo }
/// # impl BehaviorContext for Guard { type Agent = Self; type Param = (); }
/// fn shoot() -> impl BehaviorNode<Guard> {
///     seq((
///         check(|bb: &Blackboard<Guard>| bb.ammo.0 > 0),
///         leaf(|bb: &mut Blackboard<Guard>| {
///             bb.ammo.0 -= 1;
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
    /// Whether a suspended invocation resumes or reconsiders is
    /// [`BehaviorContext::entry_mode`](crate::BehaviorContext::entry_mode).
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

    /// Runs one update. `mode` comes from
    /// [`BehaviorContext::entry_mode`](crate::BehaviorContext::entry_mode).
    ///
    /// The result is not reported anywhere: at the root it says only that this
    /// invocation ended, and the next tick starts a new one. A tree that has
    /// something to say says it through `bb`.
    pub(crate) fn tick(
        &mut self,
        tree: &F::Tree,
        bb: &mut Blackboard<'_, '_, '_, '_, '_, C>,
        mode: EntryMode,
    ) {
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
            bb,
            (),
            mode,
        );
        if result != NodeResult::Running {
            self.state = None;
        }
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
