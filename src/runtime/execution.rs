use core::marker::PhantomData;

use crate::inspect::{Describe, Inspector, describe, path_id};
use crate::trace::{Trace, TraceLog, trace};
use crate::{BtNode, Entry, NodeResult};

/// Per-agent state bound to a borrowed root. Includes descendant state.
pub struct BtState<'root, N: BtNode<C, A>, C, A = ()> {
    root_node: &'root N,
    root_state: Option<N::State>,
    context: PhantomData<fn(&mut C) -> A>,
}

impl<'root, N: BtNode<C, A>, C, A> BtState<'root, N, C, A> {
    pub fn new(root_node: &'root N) -> Self {
        Self {
            root_node,
            root_state: None,
            context: PhantomData,
        }
    }

    /// A text view of the last update recorded in `log`: every node it
    /// entered, how, and what each returned. See [`Trace`].
    pub fn trace<'a>(&'a self, log: &'a TraceLog) -> Trace<'a, N, C, A> {
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
///
/// `entry` is an [`EntryMode`](crate::EntryMode), or
/// [`log.entry(mode)`](TraceLog::entry) to record the update into a
/// [`TraceLog`] the caller keeps.
pub fn update<'t, C, A, N: BtNode<C, A>>(
    root_node: &N,
    state: &mut BtState<'_, N, C, A>,
    ctx: &mut C,
    entry: impl Into<Entry<'t>>,
) -> NodeResult<A> {
    if !core::ptr::eq(root_node, state.root_node) {
        return NodeResult::error("state belongs to a different root definition");
    }
    run_root(root_node, &mut state.root_state, ctx, entry.into())
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
pub fn update_slot<'t, C, A, N: BtNode<C, A>>(
    node: &N,
    slot: &mut Option<N::State>,
    ctx: &mut C,
    entry: impl Into<Entry<'t>>,
) -> NodeResult<A> {
    run_root(node, slot, ctx, entry.into())
}

/// The drivers' body, over a plain `Entry`: converting at the edge keeps
/// release code the same as before entries could carry a trace.
fn run_root<C, A, N: BtNode<C, A>>(
    node: &N,
    slot: &mut Option<N::State>,
    ctx: &mut C,
    entry: Entry<'_>,
) -> NodeResult<A> {
    let entry = entry.start(slot.is_none());
    let result = node.update(slot.get_or_insert_with(Default::default), ctx, (), entry);
    entry.finish(&result);
    if !result.is_running() {
        *slot = None;
    }
    result
}
