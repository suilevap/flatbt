use core::marker::PhantomData;

use crate::inspect::{Describe, Inspector, describe, path_id};
use crate::trace::{Trace, TraceLog, trace};
use crate::{BtNode, Entry, EntryMode, NodeResult};

/// Per-agent state bound to a borrowed root. Includes descendant state.
pub struct BtState<'root, N: BtNode<C, A>, C, A = ()> {
    root_node: &'root N,
    root_state: Option<N::State>,
    /// Allocated by `set_trace(true)`; absent from release builds.
    #[cfg(all(debug_assertions, feature = "std"))]
    trace: Option<std::boxed::Box<TraceLog>>,
    context: PhantomData<fn(&mut C) -> A>,
}

impl<'root, N: BtNode<C, A>, C, A> BtState<'root, N, C, A> {
    pub fn new(root_node: &'root N) -> Self {
        Self {
            root_node,
            root_state: None,
            #[cfg(all(debug_assertions, feature = "std"))]
            trace: None,
            context: PhantomData,
        }
    }

    /// Records each following update, for [`trace`](Self::trace). Turning it
    /// on allocates the log once; updates reuse it. Does nothing outside
    /// debug builds with `std`. See [`crate::trace`].
    #[cfg_attr(not(all(debug_assertions, feature = "std")), allow(unused_variables))]
    pub fn set_trace(&mut self, on: bool) {
        #[cfg(all(debug_assertions, feature = "std"))]
        match (on, &self.trace) {
            (true, None) => self.trace = Some(std::boxed::Box::default()),
            (false, _) => self.trace = None,
            (true, Some(_)) => {}
        }
    }

    /// A text view of the last update: every node it entered, how, and what
    /// each returned. See [`Trace`].
    pub fn trace(&self) -> Trace<'_, N, C, A> {
        #[cfg(all(debug_assertions, feature = "std"))]
        let log = self.trace.as_deref();
        #[cfg(not(all(debug_assertions, feature = "std")))]
        let log = None;
        trace(self.root_node, self.root_state.as_ref(), log)
    }

    pub fn is_running(&self) -> bool {
        self.root_state.is_some()
    }

    /// Drops state and descendants; keeps the root binding.
    pub fn reset(&mut self) {
        self.root_state = None;
    }

    /// A text view of the running path, for logs and debugging. See
    /// [`Describe`].
    pub fn describe(&self) -> Describe<'_, N, C, A> {
        describe(self.root_node, self.root_state.as_ref())
    }

    /// A fingerprint of the running path, to log only when it changes. See
    /// [`path_id`](crate::inspect::path_id).
    pub fn path_id(&self) -> u64 {
        path_id(self.root_node, self.root_state.as_ref())
    }

    /// Reports the tree and its invocation state to `inspector`.
    pub fn inspect(&self, inspector: &mut dyn Inspector) {
        self.root_node.inspect(self.root_state.as_ref(), inspector);
    }
}

/// Runs the root and hands back what the agent is doing.
///
/// Resume follows saved selection; Evaluate revalidates from root. Fresh state
/// always enters as Evaluate. A different root logs an error and returns Failure
/// without changing state or context.
///
/// [`NodeResult::Running`] carries the act; a terminal result carries none,
/// because an agent that finished is not doing anything. Use
/// [`NodeResult::act`] to take it.
pub fn update<C, A, N: BtNode<C, A>>(
    root_node: &N,
    state: &mut BtState<'_, N, C, A>,
    ctx: &mut C,
    mode: EntryMode,
) -> NodeResult<A> {
    if !core::ptr::eq(root_node, state.root_node) {
        return NodeResult::error("state belongs to a different root definition");
    }
    #[cfg(all(debug_assertions, feature = "std"))]
    let log = state.trace.as_deref();
    #[cfg(not(all(debug_assertions, feature = "std")))]
    let log = None;
    update_slot_traced(root_node, &mut state.root_state, ctx, mode, log)
}

/// Runs a root over caller-owned invocation state: what [`update`] does, without
/// the [`BtState`] binding.
///
/// For a driver that cannot keep a `BtState` next to the tree it borrows, such
/// as an ECS component. An empty slot enters with Evaluate; a terminal result
/// clears it. The caller must pair each slot with one root.
///
/// ```
/// use flatbt::{BtNode, EntryMode, NodeResult, leaf, update_slot};
///
/// fn tree() -> impl BtNode<u32> {
///     leaf(|n: &mut u32| {
///         *n += 1;
///         if *n < 2 { NodeResult::RUNNING } else { NodeResult::Success }
///     })
/// }
///
/// let tree = tree();
/// let mut slot = None;
/// let mut n = 0;
/// assert!(update_slot(&tree, &mut slot, &mut n, EntryMode::Resume).is_running());
/// assert!(slot.is_some());
/// assert_eq!(update_slot(&tree, &mut slot, &mut n, EntryMode::Resume), NodeResult::Success);
/// assert!(slot.is_none());
/// ```
pub fn update_slot<C, A, N: BtNode<C, A>>(
    node: &N,
    slot: &mut Option<N::State>,
    ctx: &mut C,
    mode: EntryMode,
) -> NodeResult<A> {
    update_slot_traced(node, slot, ctx, mode, None)
}

/// [`update_slot`], recording the update into `log` when there is one: for a
/// driver keeping its own [`TraceLog`] beside the slot. Format it with
/// [`trace`](crate::trace::trace). Outside debug builds with `std` the log
/// records nothing.
pub fn update_slot_traced<C, A, N: BtNode<C, A>>(
    node: &N,
    slot: &mut Option<N::State>,
    ctx: &mut C,
    mode: EntryMode,
    log: Option<&TraceLog>,
) -> NodeResult<A> {
    let fresh = slot.is_none();
    let mode = if fresh { EntryMode::Evaluate } else { mode };
    #[cfg(all(debug_assertions, feature = "std"))]
    if let Some(log) = log {
        log.clear();
    }
    let entry = Entry::root(mode, log, fresh);
    let result = node.update(slot.get_or_insert_with(Default::default), ctx, (), entry);
    entry.finish(&result);
    if !result.is_running() {
        *slot = None;
    }
    result
}
