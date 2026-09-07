use crate::{BtChildren, BtNode, EntryMode, ExecutionCursor, NodeResult};

/// The next step requested by a control policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlOp {
    RunChild(usize),
    Success,
    Failure,
}

impl ControlOp {
    /// Reports a policy error to stderr and terminates this control with Failure.
    pub fn error(message: impl std::fmt::Display) -> Self {
        crate::log_error(message);
        Self::Failure
    }
}

/// Framework-owned continuation plus policy-owned state.
#[derive(Default)]
pub struct ControlState<S> {
    inner: S,
    active_child: Option<usize>,
}

/// A statically dispatched control-flow policy, separate from node execution.
///
/// State belongs to one invocation and survives suspension. Child indices must be
/// below `child_count`. Policies must eventually terminate: there is currently
/// no execution budget to stop a policy that repeatedly requests a child.
pub trait BtControl<C> {
    type State: Default + Send + 'static;

    fn begin(&self, state: &mut Self::State, ctx: &mut C, child_count: usize) -> ControlOp;

    fn child_succeeded(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        child_index: usize,
        child_count: usize,
    ) -> ControlOp;

    fn child_failed(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        child_index: usize,
        child_count: usize,
    ) -> ControlOp;
}

/// A policy paired with concrete, usually heterogeneous tuple children.
pub struct ControlNode<P, Children> {
    policy: P,
    children: Children,
}

/// Composes a custom policy with its children without erasing their types.
pub fn control<P, Children>(policy: P, children: Children) -> ControlNode<P, Children> {
    ControlNode { policy, children }
}

impl<C, P: BtControl<C>, Children: BtChildren<C>> BtNode<C> for ControlNode<P, Children> {
    type State = ControlState<P::State>;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        exec: &mut ExecutionCursor<'_>,
        mode: EntryMode,
    ) -> NodeResult {
        let mut op = match (mode, state.active_child) {
            (EntryMode::Resume, Some(index)) => ControlOp::RunChild(index),
            _ => self.policy.begin(&mut state.inner, ctx, Children::LEN),
        };
        loop {
            op = match op {
                ControlOp::Success => return NodeResult::Success,
                ControlOp::Failure => return NodeResult::Failure,
                ControlOp::RunChild(index) => {
                    if index >= Children::LEN {
                        return NodeResult::error(format_args!(
                            "control policy returned invalid child index {index} for {} children",
                            Children::LEN,
                        ));
                    }
                    match self.children.run_child(index, ctx, exec) {
                        NodeResult::Running => {
                            state.active_child = Some(index);
                            return NodeResult::Running;
                        }
                        NodeResult::Success => {
                            state.active_child = None;
                            self.policy
                                .child_succeeded(&mut state.inner, ctx, index, Children::LEN)
                        }
                        NodeResult::Failure => {
                            state.active_child = None;
                            self.policy
                                .child_failed(&mut state.inner, ctx, index, Children::LEN)
                        }
                    }
                }
            };
        }
    }
}

/// Runs children in order, stopping on the first failure.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sequence;

/// Creates a sequence. An empty sequence succeeds.
pub fn seq<Children>(children: Children) -> ControlNode<Sequence, Children> {
    control(Sequence, children)
}

impl<C> BtControl<C> for Sequence {
    type State = ();

    fn begin(&self, _: &mut (), _: &mut C, child_count: usize) -> ControlOp {
        if child_count == 0 {
            ControlOp::Success
        } else {
            ControlOp::RunChild(0)
        }
    }

    fn child_succeeded(
        &self,
        _: &mut (),
        _: &mut C,
        index: usize,
        child_count: usize,
    ) -> ControlOp {
        if index + 1 < child_count {
            ControlOp::RunChild(index + 1)
        } else {
            ControlOp::Success
        }
    }

    fn child_failed(&self, _: &mut (), _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Failure
    }
}

/// Tries children in priority order, stopping on the first success.
#[derive(Clone, Copy, Debug, Default)]
pub struct Selector;

/// Creates a selector. An empty selector fails.
pub fn select<Children>(children: Children) -> ControlNode<Selector, Children> {
    control(Selector, children)
}

impl<C> BtControl<C> for Selector {
    type State = ();

    fn begin(&self, _: &mut (), _: &mut C, child_count: usize) -> ControlOp {
        if child_count == 0 {
            ControlOp::Failure
        } else {
            ControlOp::RunChild(0)
        }
    }

    fn child_succeeded(&self, _: &mut (), _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Success
    }

    fn child_failed(&self, _: &mut (), _: &mut C, index: usize, child_count: usize) -> ControlOp {
        if index + 1 < child_count {
            ControlOp::RunChild(index + 1)
        } else {
            ControlOp::Failure
        }
    }
}
