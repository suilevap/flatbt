//! A resumable behavior tree runtime with static composition.
//!
//! ```
//! use flatbt::{BtState, NodeResult, check, leaf, seq, wait_frames};
//!
//! let tree = seq((
//!     check(|ammo: &usize| *ammo > 0),
//!     wait_frames(1),
//!     leaf(|ammo: &mut usize| {
//!         *ammo -= 1;
//!         NodeResult::Success
//!     }),
//! ));
//! let mut state = BtState::new(&tree);
//! let mut ammo = 1;
//! assert_eq!(state.update(&mut ammo), NodeResult::Running);
//! assert_eq!(state.update(&mut ammo), NodeResult::Success);
//! assert_eq!(ammo, 0);
//! ```

#![forbid(unsafe_code)]

mod children;
mod control;
mod execution;
mod leaf;
mod storage;

pub use children::BtChildren;
pub use control::{
    BtControl, ControlNode, ControlOp, ControlState, Selector, Sequence, control, select, seq,
};
pub use execution::{BtState, ExecutionCursor};
pub use leaf::{Check, Leaf, WaitFrames, check, leaf, wait_frames};

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

/// An immutable definition with separate, invocation-local state.
///
/// Fresh invocations enter as Evaluate; saved invocations enter as Resume in M1.
/// State survives only while Running. Use optional state fields to initialize
/// context-dependent data; Evaluate alone must not reset existing state.
/// Use `BtState` to drive execution; calling `update` directly bypasses it.
/// User code should report recoverable errors with `NodeResult::error`.
/// Panics in user code are not caught by the runtime.
pub trait BtNode<C> {
    type State: Default + Send + 'static;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        exec: &mut ExecutionCursor<'_>,
        mode: EntryMode,
    ) -> NodeResult;
}
