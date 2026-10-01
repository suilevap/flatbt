//! Goal-driven trees: one subtree per kind of goal, and goals that need other
//! goals first.
//!
//! [`goals`] runs a dispatch subtree for a root goal. Inside it, [`need`] runs
//! the same dispatch for a subgoal, like a function call, and returns its
//! result: `Running` while the subgoal is being worked on, with its act;
//! `Success` once it is achieved; `Failure` when it cannot be. So a blocker
//! is just a goal, and ordinary `select` and `seq` decide what to do about
//! it.
//!
//! ```
//! use flatbt::prelude::*;
//! use flatbt::goal_match;
//!
//! #[derive(Clone, PartialEq, Debug)]
//! enum Goal { Open, GetKey }
//!
//! #[derive(Default)]
//! struct World { open: bool, key: bool }
//!
//! let tree = goals::<4, _, _>(
//!     |_: &World| Goal::Open,
//!     goal_match!(|goal: &Goal| {
//!         Goal::Open => seq((
//!             need(|world: &World, _: &Goal| (!world.key).then_some(Goal::GetKey)),
//!             leaf(|world: &mut World| { world.open = true; NodeResult::Success }),
//!         )),
//!         Goal::GetKey => leaf(|world: &mut World| { world.key = true; NodeResult::Success }),
//!     }),
//! );
//! let mut state: BtState<_, _> = BtState::new(&tree);
//! let mut world = World::default();
//! assert_eq!(update(&tree, &mut state, &mut world, EntryMode::Evaluate), NodeResult::Success);
//! assert!(world.key && world.open);
//! ```
//!
//! The stack has a fixed capacity, `N` frames, laid out in the run state: no
//! heap. Each frame holds a goal and the dispatch subtree's run state for it,
//! so the run state is `N` times the dispatch subtree's.

mod dispatch;
mod need;
mod stack;

pub use dispatch::{WhenGoal, WithGoal, when_goal, with_goal};
pub use need::{Need, need};
pub use stack::{GoalCall, GoalShape, Goals, GoalsMemory, GoalsState, Subgoals, goals};
