//! A resumable behavior tree runtime with static composition.
//!
//! ```
//! use flatbt::{BtState, EntryMode, NodeResult, check, leaf, seq, update};
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
//! Core is always available. Enable `choose`, `scope`, or `action` independently
//! in Cargo.toml to add optional helpers. No features are enabled by default.
//! Depend on `flatbt-core` directly when no entry-point crate is needed.
//!
//! See `examples/resume.rs` for suspension with an application-defined node.

#![forbid(unsafe_code)]

pub use flatbt_core::*;

/// Optional catalog of ready-made nodes and policies.
#[cfg(any(feature = "action", feature = "choose"))]
pub use flatbt_nodes as nodes;
#[cfg(feature = "action")]
pub use flatbt_nodes::{ActionNode, BtAction, BtCancel, CancelOnDrop, action};
#[cfg(feature = "choose")]
pub use flatbt_nodes::{Choose, ChooseNode, choose};
/// Invocation-local storage, parameter bindings, and the scope macro.
#[cfg(feature = "scope")]
pub use flatbt_scope as scope;
