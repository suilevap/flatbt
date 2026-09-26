//! Node protocol and root invocation lifetime.

mod execution;
mod node;

pub use execution::{BtState, update, update_slot};
#[cfg(feature = "std")]
pub use node::diagnostics::{ErrorHandler, set_error_handler};
pub(crate) use node::log_error;
pub use node::{BtNode, Entry, EntryMode, NodeResult};
