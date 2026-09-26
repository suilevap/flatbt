use crate::inspect::{Inspector, NodeInfo, type_label};
use crate::trace::{Handle, TraceLog};

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
    /// Reports a diagnostic and returns Failure. Use `Failure` directly for normal
    /// outcomes. With `std`, diagnostics go to stderr unless `set_error_handler` says
    /// otherwise; without it they are discarded.
    // Cold and out of line: the controls that call this are inlined into
    // their parents, and the formatting would come along into every tick.
    #[cold]
    #[inline(never)]
    pub fn error(message: impl core::fmt::Display) -> Self {
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

#[cold]
#[inline(never)]
pub(crate) fn log_error(message: impl core::fmt::Display) {
    #[cfg(feature = "std")]
    diagnostics::report(&message);
    // Without `std` there is no stderr, and no lock to hold a handler without
    // unsafe code: the node still fails, silently.
    #[cfg(not(feature = "std"))]
    let _ = message;
}

#[cfg(feature = "std")]
pub(crate) mod diagnostics {
    use std::sync::{PoisonError, RwLock};

    /// Receives each diagnostic from [`NodeResult::error`](crate::NodeResult::error)
    /// and [`ControlOp::error`](crate::ControlOp::error). Must not panic.
    pub type ErrorHandler = fn(&dyn core::fmt::Display);

    static ERROR_HANDLER: RwLock<ErrorHandler> = RwLock::new(write_to_stderr);

    /// Sends diagnostics to `handler` instead of stderr, process-wide. Use it to
    /// route them into a logger, or to observe them in tests. Requires `std`.
    pub fn set_error_handler(handler: ErrorHandler) {
        *ERROR_HANDLER
            .write()
            .unwrap_or_else(PoisonError::into_inner) = handler;
    }

    pub(crate) fn report(message: &dyn core::fmt::Display) {
        // Copy the handler out so a slow or reentrant one holds no lock.
        let handler = *ERROR_HANDLER.read().unwrap_or_else(PoisonError::into_inner);
        handler(message);
    }

    fn write_to_stderr(message: &dyn core::fmt::Display) {
        use std::io::Write;
        // Ignore stderr errors to keep diagnostics non-panicking.
        let _ = writeln!(std::io::stderr().lock(), "[flatbt] {message}");
    }
}

/// Entry mode. Evaluate rechecks decisions without resetting state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryMode {
    Evaluate,
    Resume,
}

/// How a node is entered in one update: its [`EntryMode`], and in debug builds
/// the [trace](crate::trace) of the update, if one is on.
///
/// Drivers take anything that converts into one: an [`EntryMode`] for an
/// untraced update, or [`TraceLog::entry`] to trace it.
///
/// `Copy`. A composing node calls each child through it --
/// [`run`](Self::run), or [`run_candidate`](Self::run_candidate) for a fresh
/// invocation -- so the call is traced under the child's id. A node that
/// passes its own entry on unchanged still runs correctly, and is traced as
/// one node: calls below it are not recorded, since they would be numbered
/// from its id. In release builds an entry is the mode alone and none of this
/// does anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'t> {
    mode: EntryMode,
    trace: Handle<'t>,
}

impl<'t> Entry<'t> {
    /// An entry that records nothing, for calling a node directly.
    pub const fn new(mode: EntryMode) -> Self {
        Self {
            mode,
            trace: Handle::NONE,
        }
    }

    /// An entry recording into `log`, from [`TraceLog::entry`].
    #[inline(always)]
    pub(crate) fn traced(mode: EntryMode, log: &'t TraceLog) -> Self {
        Self {
            mode,
            trace: Handle::root(log),
        }
    }

    /// This entry, starting an update at the root of a tree of `nodes`:
    /// Evaluate for a fresh invocation, and a cleared log.
    #[inline(always)]
    pub(crate) fn start(self, fresh: bool, nodes: usize) -> Self {
        Self {
            mode: if fresh {
                EntryMode::Evaluate
            } else {
                self.mode
            },
            trace: self.trace.start(fresh, nodes),
        }
    }

    #[inline(always)]
    pub fn mode(self) -> EntryMode {
        self.mode
    }

    /// The same entry under another mode.
    #[inline(always)]
    pub fn with_mode(self, mode: EntryMode) -> Self {
        Self { mode, ..self }
    }

    /// Runs `child`, whose invocation this node holds, and records the call:
    /// what a composing node does for each child. `offset` is the child's
    /// position after this node in preorder: 1 for the first child, plus the
    /// [`NODES`](BtNode::NODES) of each child before it. Same mode.
    #[inline(always)]
    pub fn run<C, A, P, N: BtNode<C, A, P>>(
        self,
        offset: usize,
        child: &N,
        state: &mut N::State,
        ctx: &mut C,
        params: P,
    ) -> NodeResult<A> {
        let entry = self.child(offset, N::NODES);
        let result = child.update(state, ctx, params, entry);
        entry.finish(&result);
        result
    }

    /// [`run`](Self::run) for a fresh invocation of `child`: mode Evaluate,
    /// recorded as new.
    #[inline(always)]
    pub fn run_candidate<C, A, P, N: BtNode<C, A, P>>(
        self,
        offset: usize,
        child: &N,
        state: &mut N::State,
        ctx: &mut C,
        params: P,
    ) -> NodeResult<A> {
        let entry = self.candidate(offset, N::NODES);
        let result = child.update(state, ctx, params, entry);
        entry.finish(&result);
        result
    }

    /// The entry for a child whose invocation this node holds, `offset`
    /// nodes after it in preorder, with a subtree of `nodes`. Same mode. For a
    /// caller that updates the child itself; then report with
    /// [`finish`](Self::finish). [`run`](Self::run) does all three.
    #[inline(always)]
    pub fn child(self, offset: usize, nodes: usize) -> Self {
        Self {
            mode: self.mode,
            trace: self.trace.child(offset, nodes, false),
        }
    }

    /// [`child`](Self::child) for a fresh invocation: mode Evaluate, recorded
    /// as new.
    #[inline(always)]
    pub fn candidate(self, offset: usize, nodes: usize) -> Self {
        Self {
            mode: EntryMode::Evaluate,
            trace: self.trace.child(offset, nodes, true),
        }
    }

    /// Records that the node this entry was made for returned `result`.
    #[inline(always)]
    pub fn finish<A>(self, result: &NodeResult<A>) {
        self.trace.finish(self, result);
    }
}

impl From<EntryMode> for Entry<'_> {
    fn from(mode: EntryMode) -> Self {
        Self::new(mode)
    }
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
/// `entry` carries the [`EntryMode`]. Fresh entry receives Evaluate. Existing
/// entry receives Resume or Evaluate; Evaluate must not reset state. Composers
/// own descendant state, initialization, and cleanup, pass the appropriate
/// fields to child updates, and pass `entry` on, with
/// [`Entry::with_mode`] for a fresh candidate.
///
/// Drive roots with [`crate::update`] and [`crate::BtState`]. Report recoverable
/// errors with [`NodeResult::error`]. User panics propagate.
pub trait BtNode<C, A = (), P = ()> {
    type State: Default + Send + 'static;

    /// Nodes in this subtree, counted as [`inspect`](Self::inspect) reports
    /// them: 1 for a node without children. Traces name nodes by preorder
    /// index, so a composing node that reports its children counts them too.
    const NODES: usize = 1;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: P,
        entry: Entry<'_>,
    ) -> NodeResult<A>;

    /// Reports this node, and its descendants, for debugging. `state` is its
    /// invocation state when the node is on the running path.
    ///
    /// The default reports a node without children, named by its type. A
    /// composing node overrides it to report its children with the state it
    /// holds for each; see [`crate::inspect`].
    fn inspect(&self, state: Option<&Self::State>, inspector: &mut dyn Inspector) {
        inspector.node(NodeInfo::new(type_label::<Self>(), state.is_some()), |_| {});
    }
}
