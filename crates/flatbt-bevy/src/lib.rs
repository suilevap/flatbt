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
mod tree;

pub mod prelude;

#[cfg(feature = "action")]
pub use ask::{Ask, ask};
pub use context::{AgentItem, BehaviorContext, Blackboard, ParamItem, evaluate_every};
pub use plugin::{BehaviorPlugin, BehaviorSystems};
pub(crate) use tree::BehaviorTree;
pub use tree::{Behavior, BehaviorNode, EntryModeFn, TreeBuilder};

/// Matches the diagnostics FlatBT writes for recoverable errors.
pub(crate) fn log_error(message: impl core::fmt::Display) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "[flatbt] {message}");
}
