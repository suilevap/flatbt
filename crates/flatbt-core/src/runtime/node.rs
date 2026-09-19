/// Success/Failure ends an invocation; Running preserves it.
///
/// `A` is what an agent is *doing* while it runs. A node that occupies the
/// agent has to say with what, so a decision cannot exist without something
/// running, and something running cannot be silent about what it is. Trees that
/// decide nothing use the default, `A = ()`, and say [`NodeResult::RUNNING`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum NodeResult<A = ()> {
    Success,
    Failure,
    Running(A),
}

impl<A> NodeResult<A> {
    /// Logs to stderr and returns Failure. Use `Failure` directly for normal outcomes.
    pub fn error(message: impl std::fmt::Display) -> Self {
        log_error(message);
        Self::Failure
    }

    /// What the agent is doing, if this invocation is still running.
    pub fn act(self) -> Option<A> {
        match self {
            Self::Running(act) => Some(act),
            _ => None,
        }
    }

    /// Whether the invocation is still running.
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running(_))
    }
}

impl NodeResult<()> {
    /// Running, for a tree whose nodes decide nothing.
    pub const RUNNING: Self = Self::Running(());
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
/// `C` is application context; `A` is what a running invocation is doing, and
/// defaults to `()`; `P` carries parameters, including update-local borrows.
/// State survives while Running and cannot retain those borrows. Use optional
/// state fields for context-dependent initialization.
///
/// Nodes that never occupy the agent -- predicates, instant effects -- stay
/// generic over `A` and never name it, so the act type unifies from the nodes
/// that do decide and is never written out.
///
/// Fresh entry receives Evaluate. Existing entry receives Resume or Evaluate;
/// Evaluate must not reset state. Composers own descendant state, initialization,
/// and cleanup, and pass the appropriate fields to child updates.
///
/// Drive roots with [`crate::update`] and [`crate::BtState`]. Report recoverable
/// errors with [`NodeResult::error`]. User panics propagate.
pub trait BtNode<C, A = (), P = ()> {
    type State: Default + Send + 'static;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult<A>;
}
