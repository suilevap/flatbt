//! Resumable behavior trees with static dispatch and inline state.
//!
//! An update says whether the invocation ended, and if it did not, what the
//! agent is now doing. `BtState<_, _>` below says this tree decides nothing, so
//! its act type is `()`:
//!
//! ```
//! use flatbt::prelude::*;
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
//!     check(|ammo: &usize| *ammo == 0),
//!     leaf(|_: &mut usize| NodeResult::Running(Act::Reloading)),
//! ));
//! let mut state = BtState::new(&tree);
//! let mut ammo = 0;
//! let doing = update(&tree, &mut state, &mut ammo, EntryMode::Evaluate).act();
//! assert_eq!(doing, Some(Act::Reloading));
//! ```
//!
//! The crate root holds the runtime and basic composition. [`nodes`] adds
//! actions and `choose!`; [`scope`] adds invocation-local values and `scope!`.
//! [`prelude`] imports all of it.

#![forbid(unsafe_code)]

pub mod composition;
pub mod nodes;
pub mod params;
pub mod prelude;
pub mod runtime;
pub mod scope;

pub use composition::{
    BtChildren, BtControl, Check, ControlNode, ControlOp, ControlState, Guarded, Leaf, Selector,
    Sequence, check, child_state, control, guard, leaf, select, seq,
};
pub use nodes::{ActionNode, BtAction, BtCancel, CancelOnDrop, Choose, ChooseNode, action};
pub(crate) use runtime::log_error;
pub use runtime::{
    BtNode, BtState, EntryMode, ErrorHandler, NodeResult, set_error_handler, update, update_slot,
};

include!(concat!(env!("OUT_DIR"), "/child_indices.rs"));
