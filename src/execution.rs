use std::marker::PhantomData;

use crate::{BtNode, EntryMode, NodeResult, storage::FrameStorage};

/// A tree-bound execution instance. Terminal results release its active path;
/// the next update starts a fresh invocation. Dropping or resetting it drops state.
///
/// The definition is borrowed for the instance's lifetime and cannot be replaced
/// while a continuation is saved. Different instances may share one definition.
/// Normal updates follow the saved path (`ResumeCurrentPath` semantics).
pub struct BtState<'tree, N, C> {
    definition: &'tree N,
    frames: FrameStorage,
    context: PhantomData<fn(&mut C)>,
}

impl<'tree, N: BtNode<C>, C> BtState<'tree, N, C> {
    pub fn new(definition: &'tree N) -> Self {
        Self {
            definition,
            frames: FrameStorage::default(),
            context: PhantomData,
        }
    }

    pub fn update(&mut self, ctx: &mut C) -> NodeResult {
        run_node(self.definition, &mut self.frames, ctx)
    }

    pub fn is_running(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Discards the continuation through normal Rust Drop. No abort hooks run.
    pub fn reset(&mut self) {
        self.frames.clear();
    }
}

#[derive(Default)]
struct ChildSlot {
    index: Option<usize>,
    frames: FrameStorage,
}

#[derive(Default)]
struct Invocation<S> {
    // Rust drops fields in declaration order: descendants before parent state.
    child: ChildSlot,
    state: S,
}

/// Execution access for the current node invocation.
///
/// M1 uses this internally for static tuple child dispatch. Custom `BtNode`s can
/// suspend using their own state; custom composition goes through `BtControl`.
/// Dynamic child entry and root revalidation are not exposed yet.
pub struct ExecutionCursor<'a> {
    child: &'a mut ChildSlot,
}

impl ExecutionCursor<'_> {
    pub(crate) fn run_child<C, N: BtNode<C>>(
        &mut self,
        index: usize,
        node: &N,
        ctx: &mut C,
    ) -> NodeResult {
        if self.child.index != Some(index) {
            self.child.frames.clear();
            self.child.index = Some(index);
        }
        let result = run_node(node, &mut self.child.frames, ctx);
        if result != NodeResult::Running {
            self.child.index = None;
        }
        result
    }
}

fn run_node<C, N: BtNode<C>>(node: &N, storage: &mut FrameStorage, ctx: &mut C) -> NodeResult {
    let mode = if storage.is_empty() {
        EntryMode::Evaluate
    } else {
        EntryMode::Resume
    };
    let result = match storage.get_or_insert::<Invocation<N::State>>() {
        Some(frame) => {
            let mut exec = ExecutionCursor {
                child: &mut frame.child,
            };
            node.update(&mut frame.state, ctx, &mut exec, mode)
        }
        None => NodeResult::error("incompatible invocation state"),
    };
    if result != NodeResult::Running {
        storage.clear();
    }
    result
}
