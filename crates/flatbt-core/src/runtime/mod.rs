//! Node protocol and root invocation lifetime.

mod execution;
mod node;

pub use execution::{BtState, update, update_slot};
pub(crate) use node::log_error;
pub use node::{BtNode, EntryMode, ErrorHandler, NodeResult, set_error_handler};
