/// Success/Failure ends an invocation; Running preserves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum NodeResult {
    Success,
    Failure,
    Running,
}

impl NodeResult {
    /// Logs to stderr and returns Failure. Use `Failure` directly for normal outcomes.
    pub fn error(message: impl std::fmt::Display) -> Self {
        log_error(message);
        Self::Failure
    }
}

pub(crate) fn log_error(message: impl std::fmt::Display) {
    use std::io::Write;
    // Ignore stderr errors to keep diagnostics non-panicking.
    let _ = writeln!(std::io::stderr().lock(), "[flatbt] {message}");
}

/// Entry mode. Evaluate rechecks decisions without resetting state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryMode {
    Evaluate,
    Resume,
}

/// Immutable definition with owned invocation state.
///
/// `C` is application context; `P` carries parameters, including update-local
/// borrows. State survives while Running and cannot retain those borrows.
/// Use optional state fields for context-dependent initialization.
///
/// Fresh entry receives Evaluate. Existing entry receives Resume or Evaluate;
/// Evaluate must not reset state. Composers own descendant state, initialization,
/// and cleanup, and pass the appropriate fields to child updates.
///
/// Drive roots with [`crate::update`] and [`crate::BtState`]. Report recoverable
/// errors with [`NodeResult::error`]. User panics propagate.
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
