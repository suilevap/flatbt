//! Bevy ECS integration for FlatBT.
//!
//! A tree reads a blackboard component and returns what the agent is doing.
//! The tick writes that into an act component, and ordinary systems act on it.
//! Nothing here says what is in either one.
//!
//! ```
//! use bevy_app::prelude::*;
//! use bevy_ecs::prelude::*;
//! use flatbt_bevy::prelude::*;
//!
//! #[derive(Component)]
//! struct Ammo(u32);
//!
//! // 1. What the tree reads: an aggregate view of the world for this agent.
//! #[derive(Component, Default)]
//! struct Guard {
//!     ammo: u32,
//! }
//!
//! // 2. What it decides. An order to the world, not a change to it.
//! #[derive(Component, Clone, Copy, PartialEq)]
//! enum Act {
//!     Firing,
//!     Loading,
//! }
//!
//! // 3. A tree over the two. `Act` is declared nowhere but this signature.
//! //
//! // Each node that keeps the guard busy carries the condition that ends it:
//! // while a node is running, no entry mode consults anything above it, so a
//! // `check` over the firing leaf would never be asked a second time and the
//! // guard would fire on an empty magazine.
//! fn shoot() -> impl BehaviorNode<Guard, Act> {
//!     select((
//!         leaf(|guard: &mut Guard| match guard.ammo {
//!             0 => NodeResult::Failure,
//!             _ => NodeResult::Running(Act::Firing),
//!         }),
//!         leaf(|guard: &mut Guard| match guard.ammo {
//!             0 => NodeResult::Running(Act::Loading),
//!             _ => NodeResult::Success,
//!         }),
//!     ))
//! }
//!
//! // 4. The game's own systems, at the game's own rates.
//! fn gather(mut agents: Query<(&Ammo, &mut Guard)>) {
//!     for (ammo, mut guard) in agents.iter_mut() {
//!         guard.bypass_change_detection().ammo = ammo.0;
//!     }
//! }
//!
//! fn carry_out(mut agents: Query<(&Act, &mut Ammo)>) {
//!     for (act, mut ammo) in agents.iter_mut() {
//!         match act {
//!             Act::Firing => ammo.0 = ammo.0.saturating_sub(1),
//!             Act::Loading => ammo.0 = 6,
//!         }
//!     }
//! }
//!
//! // 5. Build the tree once, then spawn agents that run it.
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
//! It does not describe how a blackboard is filled or what an act means. Both
//! are the game's: a real gather is several systems at several rates -- one for
//! what is cheap, another for a raycast, another for a path query -- and what
//! an act means is however that game controls its agents.
//!
//! What the tick does own is the one thing a tree cannot: putting the decision
//! where an ECS can see it. An agent doing something carries its act component;
//! an agent whose tree ended carries none, so `Query<(&Act, &mut Transform)>`
//! is exactly the agents with a standing order. An act that only *changes* is
//! written in place, so an agent that keeps doing the same kind of thing never
//! moves archetype.
//!
//! ## Who owns what
//!
//! **The act belongs to the tick.** It writes it, and it takes it back -- when
//! the tree decides nothing, and when the agent stops running the tree at all.
//! Stopping is removing the [`Behavior`], which releases the standing order
//! rather than leaving it for the world to go on obeying;
//! [`BehaviorCommands::stop_behavior`] does that without naming a type that
//! cannot be named. A game may read the act freely, and writing to it only
//! lasts until the next tick.
//!
//! **The blackboard belongs to the game.** It is the tree's input, gathered by
//! the game's own systems. Nodes do get `&mut` to it -- it is how they leave
//! notes for each other -- but the tick passes it with change detection
//! bypassed, so those writes are invisible to `Changed<C>` and to anything
//! built on it. That is deliberate: a gather rewrites the blackboard every tick
//! anyway, and marking a whole population changed every frame would drag the
//! rest of the engine along. Anything the world should notice is an act.
//!
//! There is no way to issue an ECS command from a node. `Commands` borrows the
//! world and would put lifetimes back into every signature; the act is the way
//! out, and a system that needs `Commands` has them where it matches the act.
//!
//! [`BehaviorPlugin::parallel`] spreads agents across the task pool: a tree
//! touches its own blackboard and its own act, both disjoint per entity.
//! [`BehaviorPlugin::tick_mode`] decides, per agent per tick, whether the tree
//! is entered at all and how -- [`Tick::Skip`] leaves a suspended invocation and
//! its standing act untouched -- and [`evaluate_every`] and [`act_every`] are
//! policies for staggering that across a population.

#![forbid(unsafe_code)]

mod plugin;
mod stagger;
mod tree;

pub mod prelude;

pub use plugin::{BehaviorPlugin, BehaviorSystems};
pub use stagger::{act_every, evaluate_every};
pub use tree::{Behavior, BehaviorCommands, BehaviorNode, BehaviorTree, Tick, TickFn, TreeBuilder};
