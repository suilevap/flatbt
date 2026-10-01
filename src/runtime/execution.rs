use core::marker::PhantomData;

use crate::inspect::{Describe, Inspector, describe, path_id};
use crate::trace::{Trace, TraceLog, trace};
use crate::{BtNode, Entry, NodeResult};

/// Per-agent state bound to a borrowed root: the invocation state of the
/// running path, and every node's memory. See [`BtNode::Memory`].
pub struct BtState<'root, Node: BtNode<Context, Act>, Context, Act = ()> {
    root_node: &'root Node,
    root_state: Option<Node::State>,
    memory: Node::Memory,
    context: PhantomData<fn(&mut Context) -> Act>,
}

impl<'root, Node: BtNode<Context, Act>, Context, Act> BtState<'root, Node, Context, Act> {
    pub fn new(root_node: &'root Node) -> Self {
        Self {
            root_node,
            root_state: None,
            memory: Node::Memory::default(),
            context: PhantomData,
        }
    }

    /// A text view of the last update recorded in `log`: every node it
    /// entered, how, and what each returned. See [`Trace`].
    pub fn trace<'a>(&'a self, log: &'a TraceLog) -> Trace<'a, Node, Context, Act> {
        trace(self.root_node, self.root_state.as_ref(), &self.memory, log)
    }

    pub fn is_running(&self) -> bool {
        self.root_state.is_some()
    }

    /// Drops invocation state, so the next update enters from the root. Keeps
    /// memory and the root binding.
    pub fn reset(&mut self) {
        self.root_state = None;
    }

    /// Drops invocation state and every node's memory, as a new state for the
    /// same root would start. Keeps the root binding.
    pub fn forget(&mut self) {
        self.root_state = None;
        self.memory = Node::Memory::default();
    }

    /// Every node's memory, for a driver that saves or inspects it.
    pub fn memory(&self) -> &Node::Memory {
        &self.memory
    }

    /// A text view of the running path, for logs and debugging. See
    /// [`Describe`].
    pub fn describe(&self) -> Describe<'_, Node, Context, Act> {
        describe(self.root_node, self.root_state.as_ref(), &self.memory)
    }

    /// A fingerprint of the running path, to log only when it changes. See
    /// [`path_id`](crate::inspect::path_id).
    pub fn path_id(&self) -> u64 {
        path_id(self.root_node, self.root_state.as_ref(), &self.memory)
    }

    /// Reports the tree and its invocation state to `inspector`.
    pub fn inspect(&self, inspector: &mut dyn Inspector) {
        self.root_node
            .inspect(self.root_state.as_ref(), &self.memory, inspector);
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
pub fn update<'t, Context, Act, Node: BtNode<Context, Act>>(
    root_node: &Node,
    state: &mut BtState<'_, Node, Context, Act>,
    ctx: &mut Context,
    entry: impl Into<Entry<'t>>,
) -> NodeResult<Act> {
    if !core::ptr::eq(root_node, state.root_node) {
        return NodeResult::error("state belongs to a different root definition");
    }
    run_root(
        root_node,
        &mut state.root_state,
        &mut state.memory,
        ctx,
        entry.into(),
    )
}

/// Runs a root over caller-owned invocation state and memory: what [`update`]
/// does, without the [`BtState`] binding.
///
/// For a driver that cannot keep a `BtState` next to the tree it borrows, such
/// as an ECS component. An empty slot enters with Evaluate; a terminal result
/// clears it. `memory` starts as `Default` and is kept for the agent's
/// lifetime. The caller must pair each slot and memory with one root.
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
/// let (mut slot, mut memory) = (None, Default::default());
/// let mut n = 0;
/// assert!(update_slot(&tree, &mut slot, &mut memory, &mut n, EntryMode::Resume).is_running());
/// assert!(slot.is_some());
/// assert_eq!(
///     update_slot(&tree, &mut slot, &mut memory, &mut n, EntryMode::Resume),
///     NodeResult::Success
/// );
/// assert!(slot.is_none());
/// ```
pub fn update_slot<'t, Context, Act, Node: BtNode<Context, Act>>(
    node: &Node,
    slot: &mut Option<Node::State>,
    memory: &mut Node::Memory,
    ctx: &mut Context,
    entry: impl Into<Entry<'t>>,
) -> NodeResult<Act> {
    run_root(node, slot, memory, ctx, entry.into())
}

/// The drivers' body, over a plain `Entry`: converting at the edge keeps
/// release code the same as before entries could carry a trace.
fn run_root<Context, Act, Node: BtNode<Context, Act>>(
    node: &Node,
    slot: &mut Option<Node::State>,
    memory: &mut Node::Memory,
    ctx: &mut Context,
    entry: Entry<'_>,
) -> NodeResult<Act> {
    let entry = entry.start(slot.is_none(), Node::NODES);
    let result = node.update(
        slot.get_or_insert_with(Default::default),
        memory,
        ctx,
        (),
        entry,
    );
    entry.finish(&result);
    if !result.is_running() {
        *slot = None;
    }
    result
}
