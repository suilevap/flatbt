//! Traces of one update: which nodes it entered, how, and what each returned,
//! including branches that were tried and dropped.
//!
//! ```
//! use flatbt::prelude::*;
//! use flatbt::trace::TraceLog;
//!
//! fn has_ammo(ammo: &u32) -> bool {
//!     *ammo > 0
//! }
//!
//! let tree = select((
//!     seq((check(has_ammo), leaf(|_: &mut u32| NodeResult::Running("fire")))).named("attack"),
//!     leaf(|_: &mut u32| NodeResult::Running("reload")).named("reload"),
//! ));
//! let mut state = BtState::new(&tree);
//! let log = TraceLog::new();
//! let _ = update(&tree, &mut state, &mut 0, log.entry(EntryMode::Evaluate));
//! if flatbt::trace::ENABLED {
//!     assert_eq!(
//!         format!("{:#}", state.trace(&log)),
//!         "select → Running\n\
//!          \x20 attack (seq) → Failure\n\
//!          \x20   has_ammo (check) → Failure    ← cause\n\
//!          \x20 reload (leaf) → Running",
//!     );
//! }
//! ```
//!
//! The caller keeps a [`TraceLog`] per traced agent and passes
//! [`log.entry(mode)`](TraceLog::entry) to the driver instead of a mode; the
//! log reaches every node through its [`Entry`]. Each update clears the log
//! first and reuses its buffer.
//!
//! Recording exists only in debug builds with the `std` feature
//! ([`ENABLED`]). Elsewhere a [`TraceLog`] records nothing, [`Entry`] is the
//! mode alone, and a trace writes that it is unavailable.
//!
//! Nodes are named by their preorder index in the definition, so a log needs
//! no state to point at: dropped candidates and a tree that ended are still in
//! it. [`BtNode::NODES`] is each subtree's size, and a composing node passes
//! each child its id with [`Entry::child`] or [`Entry::candidate`].

use core::fmt;
use core::marker::PhantomData;

use crate::{BtNode, Entry, EntryMode, NodeResult};

/// Whether this build records traces: debug assertions on, and the `std`
/// feature. Otherwise every trace call does nothing.
pub const ENABLED: bool = cfg!(all(debug_assertions, feature = "std"));

/// How a call entered its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entered {
    /// A fresh invocation: a candidate, or the root with no state.
    New,
    /// The saved invocation, continued.
    Resume,
    /// The saved invocation, reconsidered.
    Evaluate,
}

/// What a call returned, without the act.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Failure,
    Running,
}

impl Outcome {
    #[cfg(all(debug_assertions, feature = "std"))]
    fn of<A>(result: &NodeResult<A>) -> Self {
        match result {
            NodeResult::Success => Self::Success,
            NodeResult::Failure => Self::Failure,
            NodeResult::Running(_) => Self::Running,
        }
    }
}

/// One node call in an update, recorded when it returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    /// The node's preorder index in the definition; the root is 0.
    pub node: usize,
    pub entered: Entered,
    pub outcome: Outcome,
}

/// The calls of one update, for one agent.
///
/// Cleared at the start of each traced update and reused, so an update
/// allocates only when it records more calls than any update before it. Past
/// its limit, recording stops and the trace says so.
pub struct TraceLog {
    #[cfg(all(debug_assertions, feature = "std"))]
    log: core::cell::RefCell<imp::Log>,
}

impl TraceLog {
    /// A log that keeps at most 4096 calls per update.
    pub fn new() -> Self {
        Self::with_limit(4096)
    }

    /// A log that keeps at most `limit` calls per update.
    #[cfg_attr(not(all(debug_assertions, feature = "std")), allow(unused_variables))]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            #[cfg(all(debug_assertions, feature = "std"))]
            log: core::cell::RefCell::new(imp::Log::new(limit)),
        }
    }

    /// The calls of the last update, in the order they returned: a copy, so
    /// the log is free for the next update. Empty unless [`ENABLED`].
    pub fn calls(&self) -> impl Iterator<Item = Call> + '_ {
        #[cfg(all(debug_assertions, feature = "std"))]
        {
            let calls = self.log.borrow().calls.clone();
            calls.into_iter()
        }
        #[cfg(not(all(debug_assertions, feature = "std")))]
        core::iter::empty()
    }

    /// An entry for a driver that records the update into this log:
    /// `update(&tree, &mut state, &mut ctx, log.entry(EntryMode::Evaluate))`.
    /// Each update clears the log first, so it holds the last one. Unless
    /// [`ENABLED`], the entry is the mode alone.
    pub fn entry(&self, mode: EntryMode) -> Entry<'_> {
        Entry::traced(mode, self)
    }

    /// Whether the last update recorded more calls than the limit.
    pub fn overflowed(&self) -> bool {
        #[cfg(all(debug_assertions, feature = "std"))]
        return self.log.borrow().overflowed;
        #[cfg(not(all(debug_assertions, feature = "std")))]
        false
    }

    #[cfg(all(debug_assertions, feature = "std"))]
    pub(crate) fn clear(&self) {
        self.log.borrow_mut().clear();
    }
}

impl Default for TraceLog {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for TraceLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraceLog").finish_non_exhaustive()
    }
}

/// What an [`Entry`] carries for tracing: in debug builds the log of this
/// update, if one is on, and the node's id; otherwise nothing.
#[derive(Clone, Copy)]
pub(crate) struct Handle<'t> {
    #[cfg(all(debug_assertions, feature = "std"))]
    to: Option<imp::To<'t>>,
    lifetime: PhantomData<&'t TraceLog>,
}

impl<'t> Handle<'t> {
    pub(crate) const NONE: Self = Self {
        #[cfg(all(debug_assertions, feature = "std"))]
        to: None,
        lifetime: PhantomData,
    };

    #[inline(always)]
    #[cfg_attr(not(all(debug_assertions, feature = "std")), allow(unused_variables))]
    pub(crate) fn root(log: &'t TraceLog) -> Self {
        Self {
            #[cfg(all(debug_assertions, feature = "std"))]
            to: Some(imp::To {
                log,
                node: 0,
                fresh: false,
            }),
            lifetime: PhantomData,
        }
    }

    /// The root's handle for a new update: its log cleared.
    #[inline(always)]
    #[cfg_attr(not(all(debug_assertions, feature = "std")), allow(unused_variables))]
    pub(crate) fn start(self, fresh: bool) -> Self {
        #[cfg(all(debug_assertions, feature = "std"))]
        if let Some(to) = self.to {
            to.log.clear();
            return Self {
                to: Some(imp::To { fresh, ..to }),
                lifetime: PhantomData,
            };
        }
        self
    }

    #[inline(always)]
    #[cfg_attr(not(all(debug_assertions, feature = "std")), allow(unused_variables))]
    pub(crate) fn child(self, offset: usize, fresh: bool) -> Self {
        Self {
            #[cfg(all(debug_assertions, feature = "std"))]
            to: self.to.map(|to| imp::To {
                node: to.node + offset,
                fresh: fresh || to.fresh,
                ..to
            }),
            lifetime: PhantomData,
        }
    }

    #[inline(always)]
    #[cfg_attr(not(all(debug_assertions, feature = "std")), allow(unused_variables))]
    pub(crate) fn finish<A>(self, entry: Entry<'_>, result: &NodeResult<A>) {
        #[cfg(all(debug_assertions, feature = "std"))]
        if let Some(to) = self.to {
            to.record(entry.mode(), Outcome::of(result));
        }
    }
}

impl fmt::Debug for Handle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(all(debug_assertions, feature = "std"))]
        if let Some(to) = self.to {
            return write!(f, "node {}", to.node);
        }
        f.write_str("untraced")
    }
}

impl PartialEq for Handle<'_> {
    fn eq(&self, other: &Self) -> bool {
        #[cfg(all(debug_assertions, feature = "std"))]
        return match (self.to, other.to) {
            (Some(a), Some(b)) => {
                core::ptr::eq(a.log, b.log) && a.node == b.node && a.fresh == b.fresh
            }
            (None, None) => true,
            _ => false,
        };
        #[cfg(not(all(debug_assertions, feature = "std")))]
        {
            let _ = other;
            true
        }
    }
}

impl Eq for Handle<'_> {}

#[cfg(all(debug_assertions, feature = "std"))]
mod imp {
    use std::vec::Vec;

    use super::{Call, Entered, Outcome, TraceLog};
    use crate::EntryMode;

    pub(super) struct Log {
        pub(super) calls: Vec<Call>,
        pub(super) limit: usize,
        pub(super) overflowed: bool,
    }

    impl Log {
        pub(super) fn new(limit: usize) -> Self {
            Self {
                calls: Vec::new(),
                limit,
                overflowed: false,
            }
        }

        pub(super) fn clear(&mut self) {
            self.calls.clear();
            self.overflowed = false;
        }
    }

    #[derive(Clone, Copy)]
    pub(super) struct To<'t> {
        pub(super) log: &'t TraceLog,
        pub(super) node: usize,
        pub(super) fresh: bool,
    }

    impl To<'_> {
        #[cold]
        #[inline(never)]
        pub(super) fn record(self, mode: EntryMode, outcome: Outcome) {
            let entered = match (self.fresh, mode) {
                (true, _) => Entered::New,
                (false, EntryMode::Resume) => Entered::Resume,
                (false, EntryMode::Evaluate) => Entered::Evaluate,
            };
            let mut log = self.log.log.borrow_mut();
            if log.calls.len() < log.limit {
                log.calls.push(Call {
                    node: self.node,
                    entered,
                    outcome,
                });
            } else {
                log.overflowed = true;
            }
        }
    }
}

/// A text view of the last update recorded in a [`TraceLog`], from
/// [`BtState::trace`](crate::BtState::trace) or [`trace`].
///
/// `{:#}` writes one line per node the update entered, indented by depth:
/// `name (kind)`, then `(resume)` or `(evaluate)` for a saved invocation --
/// fresh ones are unmarked -- then fields of the running path in braces, then
/// `→ outcome`. A node entered several times lists each outcome. `← cause`
/// marks a node that failed while none of the children it entered did.
///
/// `{}` writes the nodes still running on one line, root first.
pub struct Trace<'a, N: BtNode<C, A>, C, A = ()> {
    node: &'a N,
    state: Option<&'a N::State>,
    log: &'a TraceLog,
    context: PhantomData<fn(&mut C) -> A>,
}

/// A trace of `node` from `log`, with `state` for the running path's fields,
/// for a driver without a [`BtState`](crate::BtState), such as one using
/// [`update_slot`](crate::update_slot).
pub fn trace<'a, C, A, N: BtNode<C, A>>(
    node: &'a N,
    state: Option<&'a N::State>,
    log: &'a TraceLog,
) -> Trace<'a, N, C, A> {
    Trace {
        node,
        state,
        log,
        context: PhantomData,
    }
}

impl<N: BtNode<C, A>, C, A> fmt::Display for Trace<'_, N, C, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(all(debug_assertions, feature = "std"))]
        return text::write(
            f,
            self.log,
            |inspector| self.node.inspect(self.state, inspector),
            N::NODES,
        );
        #[cfg(not(all(debug_assertions, feature = "std")))]
        {
            let _ = (self.node, self.state, self.log);
            f.write_str("trace unavailable: needs debug assertions and std")
        }
    }
}

impl<N: BtNode<C, A>, C, A> fmt::Debug for Trace<'_, N, C, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(all(debug_assertions, feature = "std"))]
mod text {
    use core::fmt;
    use std::vec::Vec;

    use super::{Call, Entered, Outcome, TraceLog};
    use crate::inspect::{Inspector, NodeInfo};

    /// The definition's shape: each node's depth, in preorder.
    #[derive(Default)]
    struct Shape {
        depth: u32,
        nodes: Vec<u32>,
    }

    impl Inspector for Shape {
        fn enter(&mut self, _: NodeInfo<'_>) -> bool {
            self.nodes.push(self.depth);
            self.depth += 1;
            true
        }

        fn field(&mut self, _: &str, _: &dyn fmt::Debug) {}

        fn exit(&mut self) {
            self.depth -= 1;
        }
    }

    pub(super) fn write(
        f: &mut fmt::Formatter<'_>,
        log: &TraceLog,
        walk: impl Fn(&mut dyn Inspector),
        expected: usize,
    ) -> fmt::Result {
        let mut shape = Shape::default();
        walk(&mut shape);
        let nodes = shape.nodes;
        let mut calls: Vec<Vec<Call>> = (0..nodes.len()).map(|_| Vec::new()).collect();
        let log = log.log.borrow();
        let mut unknown = false;
        for call in &log.calls {
            match calls.get_mut(call.node) {
                Some(of) => of.push(*call),
                None => unknown = true,
            }
        }
        // A subtree ends at the next node no deeper than its root.
        let end = |node: usize| {
            (node + 1..nodes.len())
                .find(|&next| nodes[next] <= nodes[node])
                .unwrap_or(nodes.len())
        };
        let failed = |node: usize| {
            calls[node]
                .iter()
                .any(|call| call.outcome == Outcome::Failure)
        };
        let cause: Vec<bool> = (0..nodes.len())
            .map(|node| {
                failed(node)
                    && !(node + 1..end(node))
                        .any(|child| nodes[child] == nodes[node] + 1 && failed(child))
            })
            .collect();
        let alternate = f.alternate();
        let mut lines = Lines {
            f,
            lines: alternate,
            calls: &calls,
            cause: &cause,
            ends: (0..nodes.len()).map(end).collect(),
            next: 0,
            shown: Vec::new(),
            depth: 0,
            open: None,
            fields: false,
            written: 0,
            result: Ok(()),
        };
        walk(&mut lines);
        lines.close();
        let (written, result) = (lines.written, lines.result);
        result?;
        if written == 0 {
            f.write_str("not entered")?;
        }
        if alternate {
            if log.overflowed {
                f.write_str("\n… (limit reached)")?;
            }
            if unknown || nodes.len() != expected {
                // A custom node's `NODES` disagrees with what its `inspect`
                // reports, so ids past it are misattributed.
                write!(
                    f,
                    "\n(inspection reports {} nodes, the tree counts {expected})",
                    nodes.len()
                )?;
            }
        }
        Ok(())
    }

    struct Lines<'a, 'f> {
        f: &'a mut fmt::Formatter<'f>,
        lines: bool,
        calls: &'a [Vec<Call>],
        cause: &'a [bool],
        ends: Vec<usize>,
        /// Preorder index of the next node entered.
        next: usize,
        /// For each node entered and not exited: whether it wrote a line.
        shown: Vec<bool>,
        depth: usize,
        /// The node whose line is still open, for its outcome.
        open: Option<usize>,
        fields: bool,
        written: usize,
        result: fmt::Result,
    }

    impl Lines<'_, '_> {
        fn write(&mut self, args: fmt::Arguments<'_>) {
            if self.result.is_ok() {
                self.result = self.f.write_fmt(args);
            }
        }

        /// Ends the open line with its fields and outcomes.
        fn close(&mut self) {
            let Some(node) = self.open.take() else {
                return;
            };
            if core::mem::take(&mut self.fields) {
                self.write(format_args!("}}"));
            }
            if !self.lines {
                return;
            }
            let calls = self.calls[node].as_slice();
            match calls {
                [only] => self.write(format_args!(" → {:?}", only.outcome)),
                [first, rest @ ..] if rest.iter().all(|call| call.outcome == first.outcome) => {
                    self.write(format_args!(" → {:?} ×{}", first.outcome, calls.len()))
                }
                _ => {
                    self.write(format_args!(" →"));
                    for (index, call) in calls.iter().enumerate() {
                        let sep = if index == 0 { " " } else { ", " };
                        self.write(format_args!("{sep}{:?}", call.outcome));
                    }
                }
            }
            if self.cause[node] {
                self.write(format_args!("    ← cause"));
            }
        }

        fn begin(&mut self) -> Option<usize> {
            let node = self.next;
            self.next += 1;
            if self.calls.get(node).is_none_or(Vec::is_empty) {
                // Not entered, so neither was anything under it.
                self.next = self.ends.get(node).copied().unwrap_or(self.next);
                return None;
            }
            self.close();
            Some(node)
        }
    }

    impl Inspector for Lines<'_, '_> {
        fn enter(&mut self, node: NodeInfo<'_>) -> bool {
            let Some(id) = self.begin() else {
                return false;
            };
            let calls = &self.calls[id];
            let running = calls
                .last()
                .is_some_and(|call| call.outcome == Outcome::Running);
            let show = self.lines || running;
            if show {
                if self.lines {
                    if self.written > 0 {
                        self.write(format_args!("\n"));
                    }
                    self.write(format_args!("{:1$}", "", self.depth * 2));
                } else if self.written > 0 {
                    self.write(format_args!(" > "));
                }
                if let Some(label) = node.label {
                    self.write(format_args!("{label} => "));
                }
                match node.name {
                    Some(name) => self.write(format_args!("{name} ({})", node.kind)),
                    None => self.write(format_args!("{}", node.kind)),
                }
                if self.lines {
                    match calls[0].entered {
                        Entered::New => {}
                        Entered::Resume => self.write(format_args!(" (resume)")),
                        Entered::Evaluate => self.write(format_args!(" (evaluate)")),
                    }
                }
                self.written += 1;
                self.open = Some(id);
                self.depth += 1;
            }
            self.shown.push(show);
            true
        }

        fn field(&mut self, name: &str, value: &dyn fmt::Debug) {
            if self.open.is_none() {
                return;
            }
            let open = if core::mem::replace(&mut self.fields, true) {
                ", "
            } else {
                " {"
            };
            self.write(format_args!("{open}{name}: {value:?}"));
        }

        fn exit(&mut self) {
            self.close();
            if self.shown.pop() == Some(true) {
                self.depth -= 1;
            }
        }
    }
}
