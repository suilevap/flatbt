use std::marker::PhantomData;

use crate::{BtNode, EntryMode, NodeResult};

/// Per-agent state bound to a borrowed root. Includes descendant state.
pub struct BtState<'root, N: BtNode<C>, C> {
    root_node: &'root N,
    root_state: Option<N::State>,
    context: PhantomData<fn(&mut C)>,
}

impl<'root, N: BtNode<C>, C> BtState<'root, N, C> {
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

/// Runs the root. Resume follows saved selection; Evaluate revalidates from root.
/// Fresh state always enters as Evaluate. A different root logs an error and
/// returns Failure without changing state or context.
pub fn update<C, N: BtNode<C>>(
    root_node: &N,
    state: &mut BtState<'_, N, C>,
    ctx: &mut C,
    mode: EntryMode,
) -> NodeResult {
    if !std::ptr::eq(root_node, state.root_node) {
        return NodeResult::error("state belongs to a different root definition");
    }
    run_node(root_node, &mut state.root_state, ctx, mode)
}

/// Initializes fresh state with Evaluate; clears the slot on terminal results.
pub(crate) fn run_node<C, N: BtNode<C>>(
    node: &N,
    slot: &mut Option<N::State>,
    ctx: &mut C,
    mode: EntryMode,
) -> NodeResult {
    let mode = if slot.is_none() {
        EntryMode::Evaluate
    } else {
        mode
    };
    let result = node.update(slot.get_or_insert_with(Default::default), ctx, (), mode);
    if result != NodeResult::Running {
        *slot = None;
    }
    result
}
