//! Bevy ECS integration for FlatBT.
//!
//! Declare the world access one family of trees needs with [`BehaviorContext`],
//! add [`FlatBtPlugin`] once, then give agents a [`Behavior`] naming the tree
//! they run. Trees build and register themselves when their first agent appears.
//!
//! ```
//! use bevy_app::prelude::*;
//! use bevy_ecs::prelude::*;
//! use bevy_ecs::query::QueryData;
//! use flatbt_bevy::prelude::*;
//!
//! #[derive(Component)]
//! struct Ammo(u32);
//! #[derive(Component)]
//! struct Reloading;
//!
//! // 1. What the trees of this context may touch.
//! #[derive(QueryData)]
//! #[query_data(mutable)]
//! struct Guard {
//!     ammo: &'static mut Ammo,
//! }
//!
//! impl BehaviorContext for Guard {
//!     type Agent = Self;
//!     type Param = ();
//! }
//!
//! // 2. A tree over that context.
//! fn shoot() -> impl BehaviorNode<Guard> {
//!     select((
//!         seq((
//!             check(|bt: &Bt<Guard>| bt.ammo.0 > 0),
//!             leaf(|bt: &mut Bt<Guard>| {
//!                 bt.ammo.0 -= 1;
//!                 NodeResult::Success
//!             }),
//!         )),
//!         leaf(|bt: &mut Bt<Guard>| {
//!             bt.agent_commands().insert(Reloading);
//!             NodeResult::Success
//!         }),
//!     ))
//! }
//!
//! // 3. One plugin, then agents. No registration per tree.
//! let mut app = App::new();
//! app.add_plugins(FlatBtPlugin::new());
//! app.world_mut().spawn((Ammo(1), Behavior::for_tree(shoot)));
//!
//! app.update();
//! ```
//!
//! Nodes receive [`Bt<C>`](Bt): the agent's own components (mutable, disjoint
//! per entity), shared read-only world access, and [`Commands`] for everything
//! else. That declaration is what lets Bevy schedule the tick against other
//! systems and lets [`BehaviorPlugin::parallel`] spread agents across threads.
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
//! [`BehaviorPlugin::for_tree`] registers a tree ahead of its agents, for one
//! that needs its own schedule, ordering, run conditions, or
//! [`parallel`](BehaviorPlugin::parallel) ticking.
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

mod context;
mod plugin;
mod tree;

pub mod prelude;

pub use context::{AgentItem, BehaviorContext, Bt, ParamItem, evaluate_every};
pub use plugin::{BehaviorPlugin, BehaviorSystems, FlatBtPlugin};
pub(crate) use tree::BehaviorTree;
pub use tree::{Behavior, BehaviorNode, TreeBuilder};

/// Matches the diagnostics FlatBT writes for recoverable errors.
pub(crate) fn log_error(message: impl core::fmt::Display) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "[flatbt] {message}");
}
