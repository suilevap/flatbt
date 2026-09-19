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
//! Includes `choose`, `scope`, and `action` by default. Set `default-features = false`
//! for core only, then enable individual features as needed. The `bevy` feature
//! adds the Bevy ECS integration, re-exported here as `flatbt::bevy`.

#![forbid(unsafe_code)]

pub mod prelude;

pub use flatbt_core::*;

/// Bevy ECS integration: the agent component, the tree resource, the tick plugin.
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
