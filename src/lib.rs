//! Resumable behavior trees with static dispatch and inline state.
//!
//! # Thinking and acting
//!
//! `Blackboard` is the AI's representation of the world and its working memory.
//! Gather refreshes observed facts; nodes record reasoning and proposed plans.
//! A tree may consider several alternatives and keep what it learned. Failure
//! and branch replacement retain writes to the blackboard and node memory. Only the
//! final returned `Act` is applied to the world by the driver, after the update.
//! This contract applies to every node and builder; see [`BtNode`].
//!
//! ```
//! use flatbt::prelude::*;
//!
//! struct Observed { ammo: u32 }
//! struct Blackboard { observed: Observed, rejected_options: u32 }
//! enum Act { Fire, Reload }
//! struct World { ammo: u32 }
//!
//! let tree = select((
//!     leaf(|bb: &mut Blackboard| {
//!         if bb.observed.ammo == 0 {
//!             bb.rejected_options += 1; // Record a rejected option.
//!             NodeResult::Failure
//!         } else {
//!             NodeResult::Running(Act::Fire)
//!         }
//!     }),
//!     leaf(|_: &mut Blackboard| NodeResult::Running(Act::Reload)),
//! ));
//! let mut state = BtState::new(&tree);
//! let mut world = World { ammo: 0 };
//! let mut bb = Blackboard {
//!     observed: Observed { ammo: world.ammo }, // Gather facts before thinking.
//!     rejected_options: 0,
//! };
//!
//! let act = update(&tree, &mut state, &mut bb, EntryMode::Evaluate).act();
//! assert_eq!(bb.rejected_options, 1); // The rejected option left a thought.
//! assert_eq!(bb.observed.ammo, 0);
//! assert_eq!(world.ammo, 0);        // Thinking spent and loaded no ammunition.
//!
//! // The driver applies only the final act to the actual world.
//! match act {
//!     Some(Act::Fire) => world.ammo -= 1,
//!     Some(Act::Reload) => world.ammo = 3,
//!     None => {}
//! }
//! assert_eq!(world.ammo, 3);
//! assert_eq!(bb.observed.ammo, 0); // Still the observation from before the act.
//!
//! bb.observed = Observed { ammo: world.ammo }; // Gather the outcome next time.
//! assert_eq!(bb.observed.ammo, 3);
//! assert_eq!(bb.rejected_options, 1); // Refreshing facts preserves reasoning.
//! ```
//!
//! A tree that decides something names the act type in the nodes that decide,
//! and it unifies from there without being written out:
//!
//! ```
//! use flatbt::prelude::*;
//!
//! #[derive(Debug, PartialEq)]
//! enum Act {
//!     Reloading,
//! }
//!
//! let tree = seq((
//!     check(|observed_ammo: &usize| *observed_ammo == 0),
//!     leaf(|_: &mut usize| NodeResult::Running(Act::Reloading)),
//! ));
//! let mut state = BtState::new(&tree);
//! let mut observed_ammo = 0;
//! let doing = update(&tree, &mut state, &mut observed_ammo, EntryMode::Evaluate).act();
//! assert_eq!(doing, Some(Act::Reloading));
//! ```
//!
//! The crate root holds the runtime and basic composition. `nodes` adds
//! actions and `choose!`; `scope` adds invocation-local values and `scope!`.
//! [`prelude`] imports all of it.
//!
//! Features, both on by default:
//!
//! - `extras`: the `nodes` and `scope` modules. Without it, the crate is the
//!   runtime and composition only.
//! - `std`: diagnostics on stderr and `set_error_handler`. Without it the crate
//!   is `no_std` and allocation-free, and diagnostics are discarded.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

pub mod composition;
pub mod inspect;
#[cfg(feature = "extras")]
pub mod nodes;
pub mod params;
pub mod prelude;
pub mod runtime;
#[cfg(feature = "extras")]
pub mod scope;
pub mod trace;

pub use composition::{
    BtChildren, BtControl, Check, ControlMemory, ControlNode, ControlOp, ControlState, Guarded,
    Leaf, ReadFn, ReadsBlackboard, ReadsParams, Selector, Sequence, check, child_state, control,
    guard, leaf, select, seq,
};

#[doc(hidden)]
pub use composition::ReadsContext;
#[cfg(feature = "extras")]
pub use nodes::{
    ActionNode, ActionWait, ActionWhile, AwaitFuture, Awaiting, BtAction, BtCancel, BtClock,
    BtOrder, ByScore, CancelOnDrop, CheckWith, Choose, ChooseNode, Cooldown, Focus, GoalCall,
    Goals, IfElse, LeafWith, MapAct, Need, Ordered, ReevaluateEvery, ReevaluateWhen, Remap, Repeat,
    RepeatWhile, Retry, Shuffled, Timeout, Weighted, action, action_wait, action_while,
    await_future, by_score, check_with, cooldown, focus, force_failure, force_success, goals,
    if_else, invert, leaf_with, map_act, need, order_by, random_select, reevaluate_every,
    reevaluate_when, repeat, repeat_while, retry, shuffle_seq, shuffled, success_cooldown, timeout,
    utility, weighted, weighted_select, when_goal, with_goal,
};
pub(crate) use runtime::log_error;
pub use runtime::{BtNode, BtState, Entry, EntryMode, NodeResult, update, update_slot};
#[cfg(feature = "std")]
pub use runtime::{ErrorHandler, set_error_handler};

include!(concat!(env!("OUT_DIR"), "/child_indices.rs"));
