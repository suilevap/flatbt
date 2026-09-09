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
//! See `examples/resume.rs` for suspension with an application-defined node.

#![forbid(unsafe_code)]

mod action;
mod cancel;
mod children;
mod choose;
mod control;
mod execution;
mod leaf;
pub mod params;
pub mod scope;

pub use action::{ActionNode, BtAction, action};
pub use cancel::{BtCancel, CancelOnDrop};
pub use children::{BtChildren, child_state};
pub use choose::{Choose, ChooseNode};
pub use control::{
    BtControl, ControlNode, ControlOp, ControlState, Selector, Sequence, control, select, seq,
};
pub use execution::{BtState, update};
pub use leaf::{Check, Leaf, check, leaf};

/// The result of an invocation: terminal completion or suspension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum NodeResult {
    Success,
    Failure,
    Running,
}

impl NodeResult {
    /// Reports an execution error to stderr and returns Failure.
    /// Ordinary behavior failures should return `Failure` directly without a log.
    pub fn error(message: impl std::fmt::Display) -> Self {
        log_error(message);
        Self::Failure
    }
}

pub(crate) fn log_error(message: impl std::fmt::Display) {
    use std::io::Write;
    // A failed diagnostic write must not turn an execution error into a panic.
    let _ = writeln!(std::io::stderr().lock(), "[flatbt] {message}");
}

/// How execution enters the current invocation. Evaluate does not imply reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryMode {
    Evaluate,
    Resume,
}

/// An immutable definition with separate, statically composed state.
/// A composing node includes its descendants in State and chooses which fields
/// to pass to them. It owns initialization and cleanup of nested invocations.
/// C is application context; P is a separate parameter contract (unit by default).
/// Bindings borrow the declared inputs/outputs from an owning scope's state.
/// Parameters may contain update-local references; State must own its data.
///
/// Fresh invocations enter as Evaluate. Existing invocations can receive Resume
/// or Evaluate; Evaluate alone must not reset existing state.
/// State survives only while Running. Use optional state fields to initialize
/// context-dependent data.
/// Use the free `update` function to drive execution with a root and `BtState`.
/// Composing nodes call this method on children with their chosen state fields.
/// User code should report recoverable errors with `NodeResult::error`.
/// Panics in user code are not caught by the runtime.
pub trait BtNode<C, P = ()> {
    type State: Default + Send + 'static;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult;
}
