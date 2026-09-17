//! Optional nodes and policies. Independent features: `choose`, `action`.

#![forbid(unsafe_code)]

#[cfg(feature = "action")]
pub mod action;
#[cfg(feature = "choose")]
pub mod choose;

#[cfg(feature = "action")]
pub use action::{ActionNode, Ask, BtAction, BtCancel, CancelOnDrop, Request, action, ask};
#[cfg(feature = "choose")]
pub use choose::{Choose, ChooseNode};

// Keep macro paths valid when consumers rename dependencies.
#[doc(hidden)]
pub mod __private {
    pub use flatbt_core as core;
}
