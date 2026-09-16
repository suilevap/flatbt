//! Bevy ECS integration for FlatBT.
//!
//! Declare what the trees see with [`BehaviorContext`], register each tree with
//! [`BehaviorPlugin`], then give agents a [`Behavior`] naming the tree they run.
//!
//! ```
//! use bevy_app::prelude::*;
//! use bevy_ecs::prelude::*;
//! use bevy_ecs::query::QueryData;
//! use flatbt_bevy::prelude::*;
//!
//! #[derive(Component, PartialEq)]
//! struct Ammo(u32);
//! #[derive(Component)]
//! struct Reloading;
//!
//! // 1. What the trees of this context see: plain data, no borrows.
//! struct Guard {
//!     ammo: u32,
//! }
//!
//! // 2. The access that gathers it and writes it back.
//! #[derive(QueryData)]
//! #[query_data(mutable)]
//! struct GuardAccess {
//!     ammo: &'static mut Ammo,
//! }
//!
//! impl BehaviorContext for Guard {
//!     type Agent = GuardAccess;
//!     type Param = ();
//!     type Snapshot = Self;
//!
//!     fn read(_: Entity, agent: &GuardAccessItem, _: &()) -> Guard {
//!         Guard { ammo: agent.ammo.0 }
//!     }
//!
//!     fn write(guard: &Guard, agent: &mut GuardAccessItem) {
//!         agent.ammo.set_if_neq(Ammo(guard.ammo));
//!     }
//! }
//!
//! // 3. A tree over that context.
//! fn shoot() -> impl BehaviorNode<Guard> {
//!     select((
//!         seq((
//!             check(|bb: &Blackboard<Guard>| bb.ammo > 0),
//!             leaf(|bb: &mut Blackboard<Guard>| {
//!                 bb.ammo -= 1;
//!                 NodeResult::Success
//!             }),
//!         )),
//!         leaf(|bb: &mut Blackboard<Guard>| {
//!             bb.agent_commands().insert(Reloading);
//!             NodeResult::Success
//!         }),
//!     ))
//! }
//!
//! // 4. Build the tree once, then spawn agents that run it.
//! let mut app = App::new();
//! app.add_plugins(BehaviorPlugin::for_tree(shoot));
//! app.world_mut().spawn((Ammo(1), Behavior::for_tree(shoot)));
//!
//! app.update();
//! ```
//!
//! ## Ticking the agents yourself
//!
//! [`BehaviorPlugin`] writes the tick system out of the declaration above. That
//! is convenience, not the library: what only the library can do is hold a
//! tree's state, whose type a composed tree makes unnameable. That part is
//! [`Behavior::tick`], and it is public, so a game whose tick the declaration
//! cannot express writes its own:
//!
//! ```
//! # use bevy_app::prelude::*;
//! # use bevy_ecs::prelude::*;
//! # use bevy_ecs::query::QueryData;
//! # use flatbt_bevy::prelude::*;
//! # use flatbt_bevy::BehaviorTree;
//! # #[derive(Component, PartialEq)]
//! # struct Ammo(u32);
//! # struct Guard { ammo: u32 }
//! # #[derive(QueryData)]
//! # #[query_data(mutable)]
//! # struct GuardAccess { ammo: &'static mut Ammo }
//! # impl BehaviorContext for Guard {
//! #     type Agent = GuardAccess;
//! #     type Param = ();
//! #     type Snapshot = Self;
//! #     fn read(_: Entity, a: &GuardAccessItem, _: &()) -> Guard { Guard { ammo: a.ammo.0 } }
//! #     fn write(g: &Guard, a: &mut GuardAccessItem) { a.ammo.set_if_neq(Ammo(g.ammo)); }
//! # }
//! # fn shoot() -> impl BehaviorNode<Guard> {
//! #     leaf(|bb: &mut Blackboard<Guard>| { bb.ammo = bb.ammo.saturating_sub(1); NodeResult::Success })
//! # }
//! fn tick_guards<F: TreeBuilder<Guard>>(
//!     tree: Res<BehaviorTree<Guard, F>>,
//!     mut agents: Query<(Entity, &mut Behavior<Guard, F>, &mut Ammo)>,
//!     mut commands: Commands,
//! ) {
//!     for (entity, mut behavior, mut ammo) in agents.iter_mut() {
//!         let mut bb = Blackboard::<Guard>::new(entity, Guard { ammo: ammo.0 });
//!         behavior.tick(tree.get(), &mut bb, EntryMode::Evaluate);
//!         ammo.set_if_neq(Ammo(bb.ammo));
//!         if let Some(mut queue) = bb.take_queue() {
//!             commands.append(&mut queue);
//!         }
//!     }
//! }
//!
//! // Registering it has to be generic too: a builder's type cannot be written
//! // down, so only a function that takes one can name the system it installs.
//! fn install<F: TreeBuilder<Guard> + Copy>(app: &mut App, builder: F) {
//!     app.insert_resource(BehaviorTree::<Guard, F>::new(&builder, None))
//!         .add_systems(Update, tick_guards::<F>);
//! }
//!
//! let mut app = App::new();
//! install(&mut app, shoot);
//! # let _ = app;
//! ```
//!
//! Written out against a real context that is about forty-five lines to the
//! declaration's thirty, and it is serial. Measured over 100 000 agents it is
//! about 8% faster than the generated system on that serial path -- no function
//! pointer for the entry mode, no generic seam -- and about 1.7x slower than the
//! generated one with [`parallel`](BehaviorPlugin::parallel) on, which also
//! hands out command queues per `par_iter` batch, skips the write-back for a
//! tree that only read, and cannot be misordered. So write your own when you
//! need something the declaration cannot say, and expect to re-earn the rest.
//!
//! Nodes receive [`Blackboard<C>`](Blackboard): the snapshot
//! [`BehaviorContext::read`] gathered, plus what the tree may defer to the
//! world. It owns its data, so it has no lifetimes, a node signature names
//! nothing but `Blackboard<Guard>`, and a tree can be exercised without a
//! [`World`](bevy_ecs::world::World). [`Agent`](BehaviorContext::Agent) and
//! [`Param`](BehaviorContext::Param) declare the access `read` and `write` use,
//! which is what lets Bevy schedule the tick against other systems and lets
//! [`BehaviorPlugin::parallel`] spread agents across threads.
//!
//! # What lives where
//!
//! A tree is an immutable definition, so it is built once into
//! a resource of its own. [`Behavior<C, F>`](Behavior)
//! holds only what is per-agent: the saved state of a suspended invocation,
//! sized exactly for that tree. Nothing is allocated, nothing is reference
//! counted, and dispatch stays static.
//!
//! Neither type parameter is ever written out. Both come from the builder
//! function, which is the tree's name: identity is the builder rather than the
//! tree type, so two builders returning the same tree type with different node
//! configuration stay separate.
//!
//! [`BehaviorPlugin`] builds the tree when the app is built, so the tick is in
//! place before any agent exists and any schedule will do -- including one the
//! game runs itself. It also carries the tree's schedule, ordering, run
//! conditions, [`parallel`](BehaviorPlugin::parallel) ticking and its own
//! [`entry_mode`](BehaviorPlugin::entry_mode).
//!
//! Trees are written with FlatBT's own API. `seq`, `select`, `check`, `leaf`,
//! `choose!`, `scope!`, `action` and custom [`BtNode`](flatbt_core::BtNode)s all
//! take this context as written, with no Bevy-specific constructors. A subtree
//! is a function returning a node, so it composes into any tree by being called.
//!
//! Agents resume by default, the cheap path: decisions already taken stand.
//! [`BehaviorContext::entry_mode`] decides per agent per tick when a tree should
//! reconsider instead — on a timer, on a changed resource, on a perception
//! component — reading the same world the tree declared. [`evaluate_every`]
//! answers it on a period without putting a whole population on one frame.
//!
//! Stopping agents is the game's own: a run condition on [`BehaviorSystems`]
//! halts every tree, and a guard at the root of a tree halts that one.
//!
//! [`Commands`]: bevy_ecs::prelude::Commands

#![forbid(unsafe_code)]

#[cfg(feature = "action")]
mod ask;
mod context;
mod plugin;
mod stagger;
mod tree;

pub mod prelude;

#[cfg(feature = "action")]
pub use ask::{Ask, ask};
pub use context::{AgentItem, BehaviorContext, Blackboard, EntityCommandQueue, ParamItem};
pub use plugin::{BehaviorPlugin, BehaviorSystems};
pub use stagger::evaluate_every;
pub use tree::BehaviorTree;
pub use tree::{Behavior, BehaviorNode, EntryModeFn, TreeBuilder};

/// Matches the diagnostics FlatBT writes for recoverable errors.
#[cfg(debug_assertions)]
pub(crate) fn log_error(message: impl core::fmt::Display) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "[flatbt] {message}");
}
