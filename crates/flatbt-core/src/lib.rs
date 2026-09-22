//! Resumable behavior trees with static dispatch and inline state.
//!
//! An update says whether the invocation ended, and if it did not, what the
//! agent is now doing. A tree whose nodes decide nothing uses the default act
//! type, `()`, which is what `BtState<_, _>` says here:
//!
//! ```
//! use flatbt_core::{BtState, EntryMode, NodeResult, check, leaf, seq, update};
//!
//! let tree = seq((
//!     check(|ammo: &usize| *ammo > 0),
//!     leaf(|ammo: &mut usize| {
//!         *ammo -= 1;
//!         NodeResult::Success
//!     }),
//! ));
//! let mut state: BtState<_, _> = BtState::new(&tree);
//! let mut ammo = 1;
//! assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Success);
//! assert_eq!(ammo, 0);
//! ```
//!
//! A tree that decides something names the act type in its nodes, and it
//! unifies from there:
//!
//! ```
//! use flatbt_core::{BtState, EntryMode, NodeResult, check, leaf, seq, update};
//!
//! #[derive(Debug, PartialEq)]
//! enum Act {
//!     Reloading,
//! }
//!
//! let tree = seq((
//!     check(|ammo: &usize| *ammo == 0),
//!     leaf(|_: &mut usize| NodeResult::Running(Act::Reloading)),
//! ));
//! let mut state = BtState::new(&tree);
//! let mut ammo = 0;
//! let doing = update(&tree, &mut state, &mut ammo, EntryMode::Evaluate).act();
//! assert_eq!(doing, Some(Act::Reloading));
//! ```
//!
//! Independent of the optional node catalog and scope DSL.

#![forbid(unsafe_code)]

pub mod composition;
pub mod params;
pub mod runtime;

pub use composition::{
    BtChildren, BtControl, Check, ControlNode, ControlOp, ControlState, Guarded, Leaf, Selector,
    Sequence, check, child_state, control, guard, leaf, select, seq,
};
pub(crate) use runtime::log_error;
pub use runtime::{BtNode, BtState, EntryMode, NodeResult, update, update_slot};

include!(concat!(env!("OUT_DIR"), "/child_indices.rs"));
