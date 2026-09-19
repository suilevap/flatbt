use std::marker::PhantomData;

use crate::{BtNode, EntryMode, NodeResult};

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

    pub fn is_running(&self) -> bool {
        self.root_state.is_some()
    }

    /// Drops state and descendants; keeps the root binding.
    pub fn reset(&mut self) {
        self.root_state = None;
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
    if !std::ptr::eq(root_node, state.root_node) {
        return NodeResult::error("state belongs to a different root definition");
    }
    run_node(root_node, &mut state.root_state, ctx, mode)
}

/// Initializes fresh state with Evaluate; clears the slot on terminal results.
pub(crate) fn run_node<C, A, N: BtNode<C, A>>(
    node: &N,
    slot: &mut Option<N::State>,
    ctx: &mut C,
    mode: EntryMode,
) -> NodeResult<A> {
    let mode = if slot.is_none() {
        EntryMode::Evaluate
    } else {
        mode
    };
    let result = node.update(slot.get_or_insert_with(Default::default), ctx, (), mode);
    if !result.is_running() {
        *slot = None;
    }
    result
}
