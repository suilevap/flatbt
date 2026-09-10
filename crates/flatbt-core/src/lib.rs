//! Resumable behavior trees with static dispatch and inline state.
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
//! let mut state = BtState::new(&tree);
//! let mut ammo = 1;
//! assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Success);
//! assert_eq!(ammo, 0);
//! ```
//!
//! Independent of the optional node catalog and scope DSL.

#![forbid(unsafe_code)]

pub mod composition;
pub mod params;
pub mod runtime;

pub use composition::{
    BtChildren, BtControl, Check, ControlNode, ControlOp, ControlState, Leaf, Selector, Sequence,
    check, child_state, control, leaf, select, seq,
};
pub(crate) use runtime::log_error;
pub use runtime::{BtNode, BtState, EntryMode, NodeResult, update};

include!(concat!(env!("OUT_DIR"), "/child_indices.rs"));
