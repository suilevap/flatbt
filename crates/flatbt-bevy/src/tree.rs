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

/// What a tick does with one agent, decided from its blackboard.
///
/// [`Skip`](Tick::Skip) is the one a tree cannot express for itself. A guard at
/// the root does not hold a suspended tree still: `Resume` re-enters the active
/// child directly, so a child above it is never consulted, and `seq` continues
/// its active child on `Evaluate` too. Even a `select`, which does rescan, runs
/// its standing branch when the candidate above fails -- failing a candidate is
/// how a tree *redirects*, not how it stops. So "do not run this agent at all"
/// has to be said before the tree is entered.
///
/// It matters when the work is elsewhere. An agent whose action is being
/// carried out by ordinary systems over the next hundred frames has nothing to
/// decide until that ends, and reconsidering it every frame is the cost the
/// whole population pays. Over 200 000 agents on a 4-core Xeon, with nine in
/// ten having nothing to decide:
///
/// | | serial tick |
/// | --- | --- |
/// | every agent enters the tree | 2.19-2.24 ms |
/// | nine in ten fail a guard at the root | 1.49-1.50 ms |
/// | nine in ten are `Skip`ped | 0.62-0.63 ms |
///
/// And the guard row is the optimistic one: it only works at all for an agent
/// entering as `Evaluate` with nothing suspended below the guard.
///
/// See `tests/entry.rs` for what each variant does to a suspended invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Do not enter the tree. The suspended invocation is left exactly as it
    /// was, so a later tick resumes into the same node.
    Skip,
    /// Enter, continuing where the invocation left off.
    Resume,
    /// Enter, reconsidering from the root.
    Evaluate,
}

impl From<EntryMode> for Tick {
    fn from(mode: EntryMode) -> Self {
        match mode {
            EntryMode::Resume => Tick::Resume,
            EntryMode::Evaluate => Tick::Evaluate,
        }
    }
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

/// What a tick does with one agent, given its blackboard.
pub type TickFn<C> = fn(&C) -> Tick;

/// The one tree named by `F`, built once and shared by every agent running it.
///
/// Inserted by [`BehaviorPlugin`](crate::BehaviorPlugin). Trees are immutable
/// definitions, so they belong in a resource rather than copied into each agent.
/// Public so a game ticking its agents itself reads the tree the same way the
/// generated system does; see [`Behavior::tick`].
#[derive(Resource)]
pub struct BehaviorTree<C: Send + Sync + 'static, F: TreeBuilder<C>> {
    tree: F::Tree,
    tick_mode: TickFn<C>,
    // Load-bearing: it keeps `C` a direct field use. Reached only through the
    // `F::Tree` projection, `C` sends the monomorphization collector through
    // every blanket impl behind it and over the recursion limit.
    context: PhantomData<fn() -> C>,
}

impl<C: Send + Sync + 'static, F: TreeBuilder<C>> BehaviorTree<C, F> {
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
