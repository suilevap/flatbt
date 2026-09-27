use crate::inspect::Inspector;
use crate::params::ParamShape;
use crate::{BtNode, ControlOp, Entry, NodeResult};

/// Static tuple dispatch with at most one active child.
/// Generated through `FLATBT_MAX_CHILDREN` (default 32).
///
/// `S` is the shape of the parameters every child receives; see `ParamShape`.
pub trait BtChildren<C, A = (), S: ParamShape = ()> {
    type State: Default + Send + 'static;
    const LEN: usize;

    /// Reads selection from the saved state variant.
    fn active_child_index(&self, state: &Self::State) -> Option<usize>;

    /// Runs child `first`, then, while `next` answers `RunChild(i + 1)` for the
    /// child `i` that just ended, the child after it, without leaving this call.
    /// `next` receives whether the child succeeded. Returns `Ok` with a Running
    /// result, or `Err` with the first answer that is not the following child.
    ///
    /// Each child updates its saved state in place, or enters a fresh candidate
    /// with Evaluate. Terminal candidates preserve saved state; Running
    /// candidates replace it. A terminal saved child clears selection. An
    /// index at or past `LEN` runs nothing and comes back as `Err(RunChild)`.
    fn run_from(
        &self,
        state: &mut Self::State,
        first: usize,
        ctx: &mut C,
        params: &mut S::Value<'_>,
        entry: Entry<'_>,
        next: &mut impl FnMut(&mut C, usize, bool) -> ControlOp,
    ) -> Result<NodeResult<A>, ControlOp>;

    /// Reports each child in order, with its state when it is the saved one.
    /// See [`BtNode::inspect`].
    fn inspect_children(&self, state: Option<&Self::State>, inspector: &mut dyn Inspector);

    /// Nodes in all children together; see [`BtNode::NODES`]. Child `i` is
    /// called with an entry whose offset is 1 plus the `NODES` of the children
    /// before it, so a control's own id comes first.
    const NODES: usize;
}

impl<C, A, S: ParamShape> BtChildren<C, A, S> for () {
    type State = ();
    const LEN: usize = 0;
    const NODES: usize = 0;

    fn active_child_index(&self, _: &()) -> Option<usize> {
        None
    }

    fn run_from(
        &self,
        _: &mut (),
        first: usize,
        _: &mut C,
        _: &mut S::Value<'_>,
        _: Entry<'_>,
        _: &mut impl FnMut(&mut C, usize, bool) -> ControlOp,
    ) -> Result<NodeResult<A>, ControlOp> {
        Err(ControlOp::RunChild(first))
    }

    fn inspect_children(&self, _: Option<&()>, _: &mut dyn Inspector) {}
}

macro_rules! tuple_children {
    (@generate_impl $state:ident; $($index:tt $node:ident $variant:ident $child_state:ident),+) => {
        /// One active child state; the variant encodes its index.
        #[derive(Default)]
        pub enum $state<$($node),+> {
            #[default]
            Empty,
            $($variant($node),)+
        }

        impl<C, A, S: ParamShape, $($node, $child_state),+> BtChildren<C, A, S> for ($($node,)+)
        where
            $($node: for<'a> BtNode<C, A, S::Value<'a>, State = $child_state>,
            $child_state: Default + Send + 'static,)+
        {
            type State = $state<$($child_state),+>;
            const LEN: usize = [$(stringify!($node)),+].len();
            const NODES: usize = 0 $(+ <$node as BtNode<C, A, S::Value<'static>>>::NODES)+;

            #[inline(always)]
            fn active_child_index(&self, state: &Self::State) -> Option<usize> {
                match state {
                    $state::Empty => None,
                    $($state::$variant(_) => Some($index),)+
                }
            }

            #[inline(always)]
            fn run_from(
                &self,
                state: &mut Self::State,
                first: usize,
                ctx: &mut C,
                params: &mut S::Value<'_>,
                entry: Entry<'_>,
                next: &mut impl FnMut(&mut C, usize, bool) -> ControlOp,
            ) -> Result<NodeResult<A>, ControlOp> {
                // One block per child, in order. When the policy asks for the
                // following child, control falls into the next block; for a
                // policy the compiler can see through, such as `Sequence`, the
                // index is a constant and the chain is straight-line code.
                let mut index = first;
                // Each child's preorder offset from the control, for traces.
                let offsets = const {
                    let nodes = [$(<$node as BtNode<C, A, S::Value<'static>>>::NODES),+];
                    let mut offsets = [$($index * 0),+];
                    let mut at = 1;
                    let mut child = 0;
                    while child < nodes.len() {
                        offsets[child] = at;
                        at += nodes[child];
                        child += 1;
                    }
                    offsets
                };
                $(
                    if index == $index {
                        let offset = offsets[$index];
                        let result = if let $state::$variant(active) = state {
                            let entry = entry.child(offset, <$node as BtNode<C, A, S::Value<'static>>>::NODES);
                            let result = self.$index.update(active, ctx, S::reborrow(params), entry);
                            entry.finish(&result);
                            if !result.is_running() {
                                *state = $state::Empty;
                            }
                            result
                        } else {
                            // Preserve the old variant until this candidate is selected.
                            let mut candidate = $child_state::default();
                            let entry = entry.candidate(offset, <$node as BtNode<C, A, S::Value<'static>>>::NODES);
                            let result = self.$index.update(&mut candidate, ctx, S::reborrow(params), entry);
                            entry.finish(&result);
                            if result.is_running() {
                                *state = $state::$variant(candidate);
                            }
                            result
                        };
                        let op = match result {
                            // The act comes from whichever child actually ran.
                            running @ NodeResult::Running(_) => return Ok(running),
                            NodeResult::Success => next(ctx, $index, true),
                            NodeResult::Failure => next(ctx, $index, false),
                        };
                        match op {
                            ControlOp::RunChild(following) if following == $index + 1 => index = following,
                            op => return Err(op),
                        }
                    }
                )+
                Err(ControlOp::RunChild(index))
            }

            fn inspect_children(&self, state: Option<&Self::State>, inspector: &mut dyn Inspector) {
                $(
                    let active = match state {
                        Some($state::$variant(active)) => Some(active),
                        _ => None,
                    };
                    BtNode::<C, A, S::Value<'_>>::inspect(&self.$index, active, inspector);
                )+
            }
        }
    };
    (@generate_prefix [$($done_index:tt $done_node:ident $done_variant:ident $done_child_state:ident,)*] $state:ident $index:tt $node:ident $variant:ident $child_state:ident $(, $tail_state:ident $tail_index:tt $tail_node:ident $tail_variant:ident $tail_child_state:ident)*) => {
        tuple_children!(@generate_impl $state; $($done_index $done_node $done_variant $done_child_state,)* $index $node $variant $child_state);
        tuple_children!(@generate_prefix [$($done_index $done_node $done_variant $done_child_state,)* $index $node $variant $child_state,] $($tail_state $tail_index $tail_node $tail_variant $tail_child_state),*);
    };
    (@generate_prefix [$($done:tt)*]) => {};
}

/// Generated tuple state enums, parameterized by child state types.
pub mod child_state {
    use super::{BtChildren, BtNode, ControlOp, Entry, Inspector, NodeResult, ParamShape};

    include!(concat!(env!("OUT_DIR"), "/tuple_children.rs"));
}
