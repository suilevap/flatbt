//! Goal-driven trees: one subtree per kind of goal, and goals that need other
//! goals first.
//!
//! [`goals`] keeps a stack of goals and runs a dispatch subtree for the one
//! on top. Inside it, [`need`] pushes a subgoal: the current goal's run stops
//! and the subgoal runs next. When a goal's subtree succeeds or fails, the goal
//! is popped and the one below runs again from its start, where its `need`
//! returns that result.
//! So a blocker is just a goal, and ordinary `select` and `seq` decide what to
//! do about it.
//!
//! ```
//! use flatbt::prelude::*;
//! use flatbt::goal_match;
//!
//! #[derive(Clone, PartialEq, Debug)]
//! enum Goal { PlanRoute, FindWaypoint }
//!
//! #[derive(Default)]
//! struct Blackboard { route_planned: bool, waypoint_found: bool }
//!
//! let tree = goals::<4, _, _>(
//!     |_: &Blackboard| Goal::PlanRoute,
//!     goal_match!(|goal: &Goal| {
//!         Goal::PlanRoute => seq((
//!             need(|bb: &Blackboard, _: &Goal| (!bb.waypoint_found).then_some(Goal::FindWaypoint)),
//!             leaf(|bb: &mut Blackboard| { bb.route_planned = true; NodeResult::Success }),
//!         )),
//!         Goal::FindWaypoint => leaf(|bb: &mut Blackboard| { bb.waypoint_found = true; NodeResult::Success }),
//!     }),
//! );
//! let mut state: BtState<_, _> = BtState::new(&tree);
//! let mut bb = Blackboard::default();
//! assert_eq!(update(&tree, &mut state, &mut bb, EntryMode::Evaluate), NodeResult::Success);
//! assert!(bb.waypoint_found && bb.route_planned);
//! ```
//!
//! No recursion and no heap: the run state is `DEPTH` goals and one run state of
//! the dispatch subtree, for the goal on top.

mod dispatch;
mod need;
mod stack;

pub use dispatch::{WhenGoal, WithGoal, when_goal, with_goal};
pub use need::{Need, need};
pub use stack::{GoalCall, GoalDone, GoalShape, Goals, GoalsState, NotDone, goals};
