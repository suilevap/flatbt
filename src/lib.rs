//! Resumable behavior trees with static dispatch and inline state.
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
//! let mut state = BtState::new(&tree);
//! let mut ammo = 1;
//! assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Success);
//! assert_eq!(ammo, 0);
//! ```
//!
//! Includes `choose`, `scope`, and `action` by default. Set `default-features = false`
//! for core only, then enable individual features as needed. The `bevy` feature adds
//! [Bevy ECS](bevy) integration.

#![forbid(unsafe_code)]

pub mod prelude;

pub use flatbt_core::*;

/// Bevy ECS integration: context declaration, agent component, tick plugin.
#[cfg(feature = "bevy")]
pub use flatbt_bevy as bevy;
/// Optional nodes and policies.
#[cfg(any(feature = "action", feature = "choose"))]
pub use flatbt_nodes as nodes;
#[cfg(feature = "action")]
pub use flatbt_nodes::{ActionNode, BtAction, BtCancel, CancelOnDrop, action};
#[cfg(feature = "choose")]
pub use flatbt_nodes::{Choose, ChooseNode, choose};
/// Invocation-local storage, bindings, and `scope!`.
#[cfg(feature = "scope")]
pub use flatbt_scope as scope;
