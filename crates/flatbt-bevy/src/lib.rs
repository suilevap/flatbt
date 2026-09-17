//! Bevy ECS integration for FlatBT.
//!
//! A tree's blackboard is an ordinary component. The game fills it with
//! whatever systems it likes, the tree reads and writes it, and the game reads
//! the decisions back out. Nothing here declares what is in it.
//!
//! ```
//! use bevy_app::prelude::*;
//! use bevy_ecs::prelude::*;
//! use flatbt_bevy::prelude::*;
//!
//! #[derive(Component)]
//! struct Ammo(u32);
//!
//! // 1. The blackboard: what the tree reads, and what it decides.
//! #[derive(Component, Default)]
//! struct Guard {
//!     ammo: u32,
//!     reload: bool,
//!     fire: bool,
//! }
//!
//! // 2. A tree over it. A plain struct, so no lifetimes anywhere.
//! fn shoot() -> impl BehaviorNode<Guard> {
//!     select((
//!         seq((
//!             check(|guard: &Guard| guard.ammo > 0),
//!             leaf(|guard: &mut Guard| {
//!                 guard.fire = true;
//!                 NodeResult::Success
//!             }),
//!         )),
//!         leaf(|guard: &mut Guard| {
//!             guard.reload = true;
//!             NodeResult::Success
//!         }),
//!     ))
//! }
//!
//! // 3. The game's own systems, at the game's own rates.
//! fn gather(mut agents: Query<(&Ammo, &mut Guard)>) {
//!     for (ammo, mut guard) in agents.iter_mut() {
//!         let guard = guard.bypass_change_detection();
//!         guard.ammo = ammo.0;
//!         guard.fire = false;
//!         guard.reload = false;
//!     }
//! }
//!
//! fn carry_out(mut agents: Query<(&Guard, &mut Ammo)>) {
//!     for (guard, mut ammo) in agents.iter_mut() {
//!         if guard.fire && ammo.0 > 0 {
//!             ammo.0 -= 1;
//!         }
//!         if guard.reload {
//!             ammo.0 = 6;
//!         }
//!     }
//! }
//!
//! // 4. Build the tree once, then spawn agents that run it.
//! let mut app = App::new();
//! app.add_plugins(BehaviorPlugin::for_tree(shoot))
//!     .add_systems(Update, gather.before(BehaviorSystems))
//!     .add_systems(Update, carry_out.after(BehaviorSystems));
//! app.world_mut()
//!     .spawn((Ammo(1), Guard::default(), Behavior::for_tree(shoot)));
//!
//! app.update();
//! ```
//!
//! ## What is here, and what is not
//!
//! The library holds a tree and its per-agent state, because a composed tree's
//! invocation state is nested control state over closures and its type cannot
//! be written down. [`Behavior`] and [`TreeBuilder`] exist for that, and so
//! does the generic registration -- a builder's type cannot be named either.
//!
//! It does not describe how a blackboard is filled or what a decision means.
//! Both are the game's: a real gather is several systems at several rates --
//! one for what is cheap, another for a raycast, another for a path query --
//! and what a tree writes is however that game controls its agents, which no
//! library can guess. [`BehaviorPlugin`] registers a tick and nothing else, and
//! [`Behavior::tick`] is public for a game that wants to register its own.
//!
//! [`BehaviorPlugin::parallel`] spreads agents across the task pool: a tree
//! touches one component, its own blackboard, which is disjoint per entity.
//! [`BehaviorPlugin::entry_mode`] decides per agent per tick whether a suspended
//! invocation reconsiders, and [`evaluate_every`] is a policy for staggering
//! that across a population.

#![forbid(unsafe_code)]

mod plugin;
mod stagger;
mod tree;

pub mod prelude;

pub use plugin::{BehaviorPlugin, BehaviorSystems};
pub use stagger::evaluate_every;
pub use tree::{Behavior, BehaviorNode, BehaviorTree, EntryModeFn, TreeBuilder};
