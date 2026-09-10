//! Optional ready-made nodes and policies for FlatBT.
//! Enable `choose` or `action` independently. Future policies belong here.

#![forbid(unsafe_code)]

#[cfg(feature = "action")]
pub mod action;
#[cfg(feature = "choose")]
pub mod choose;

#[cfg(feature = "action")]
pub use action::{ActionNode, BtAction, BtCancel, CancelOnDrop, action};
#[cfg(feature = "choose")]
pub use choose::{Choose, ChooseNode};

// Exported macros must also work when this dependency is renamed by a consumer.
#[doc(hidden)]
pub mod __private {
    pub use flatbt_core as core;
}
