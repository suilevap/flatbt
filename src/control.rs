use crate::{BtChildren, BtNode, EntryMode, NodeResult};

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

/// Statically composed child states, policy-local state, and selection metadata.
#[derive(Default)]
pub struct ControlState<S, ChildrenState> {
    // Field order releases descendants before the policy state.
    children: ChildrenState,
    inner: S,
    active_child_index: Option<usize>,
}

/// A statically dispatched control-flow policy, separate from node execution.
///
/// State belongs to one invocation and survives suspension. Child indices must be
/// below `child_count`. Policies must eventually terminate: there is currently
/// no execution budget to stop a policy that repeatedly requests a child.
pub trait BtControl<C> {
    type State: Default + Send + 'static;

    /// Revalidates this control's decision. `active_child_index` is ControlNode-owned
    /// continuation metadata, passed by value so the policy cannot overwrite it.
    /// Sequence preserves it; Selector deliberately starts a new priority scan.
    /// Normal Resume follows the active child without calling this method.
    fn begin(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp;

    /// Handles the child that just succeeded, after its invocation has ended.
    fn child_succeeded(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        completed_child_index: usize,
        child_count: usize,
    ) -> ControlOp;

    /// Handles the child that just failed, after its invocation has ended.
    fn child_failed(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        completed_child_index: usize,
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
    type State = ControlState<P::State, Children::State>;

    fn update(&self, state: &mut Self::State, ctx: &mut C, mode: EntryMode) -> NodeResult {
        let active_child_index = state.active_child_index;
        let mut op = match (mode, active_child_index) {
            (EntryMode::Resume, Some(child_index)) => ControlOp::RunChild(child_index),
            _ => self
                .policy
                .begin(&mut state.inner, ctx, active_child_index, Children::LEN),
        };
        loop {
            op = match op {
                ControlOp::Success => return NodeResult::Success,
                ControlOp::Failure => return NodeResult::Failure,
                ControlOp::RunChild(child_index) => {
                    if child_index >= Children::LEN {
                        return NodeResult::error(format_args!(
                            "control policy returned invalid child index {child_index} for {} children",
                            Children::LEN,
                        ));
                    }
                    // A terminal fresh candidate must preserve the saved selection:
                    // the policy may still return to it later in this update.
                    let existing = state.active_child_index == Some(child_index);
                    match self
                        .children
                        .run_child(&mut state.children, child_index, ctx, mode)
                    {
                        NodeResult::Running => {
                            if let Some(previous) = state.active_child_index
                                && previous != child_index
                            {
                                self.children.reset_child(&mut state.children, previous);
                            }
                            state.active_child_index = Some(child_index);
                            return NodeResult::Running;
                        }
                        NodeResult::Success => {
                            if existing {
                                state.active_child_index = None;
                            }
                            self.policy.child_succeeded(
                                &mut state.inner,
                                ctx,
                                child_index,
                                Children::LEN,
                            )
                        }
                        NodeResult::Failure => {
                            if existing {
                                state.active_child_index = None;
                            }
                            self.policy.child_failed(
                                &mut state.inner,
                                ctx,
                                child_index,
                                Children::LEN,
                            )
                        }
                    }
                }
            };
        }
    }
}

/// Runs children in order, stopping on the first failure. Re-evaluation preserves
/// the active child instead of replaying completed children.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sequence;

/// Creates a sequence. An empty sequence succeeds.
pub fn seq<Children>(children: Children) -> ControlNode<Sequence, Children> {
    control(Sequence, children)
}

impl<C> BtControl<C> for Sequence {
    type State = ();

    fn begin(
        &self,
        _: &mut (),
        _: &mut C,
        active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp {
        if child_count == 0 {
            ControlOp::Success
        } else {
            ControlOp::RunChild(active_child_index.unwrap_or(0))
        }
    }

    fn child_succeeded(
        &self,
        _: &mut (),
        _: &mut C,
        completed_child_index: usize,
        child_count: usize,
    ) -> ControlOp {
        if completed_child_index + 1 < child_count {
            ControlOp::RunChild(completed_child_index + 1)
        } else {
            ControlOp::Success
        }
    }

    fn child_failed(
        &self,
        _: &mut (),
        _: &mut C,
        _completed_child_index: usize,
        _child_count: usize,
    ) -> ControlOp {
        ControlOp::Failure
    }
}

/// Tries children in priority order, stopping on the first success. Re-evaluation
/// starts at child zero; Resume follows the saved child without rescanning.
#[derive(Clone, Copy, Debug, Default)]
pub struct Selector;

/// Creates a selector. An empty selector fails.
pub fn select<Children>(children: Children) -> ControlNode<Selector, Children> {
    control(Selector, children)
}

impl<C> BtControl<C> for Selector {
    type State = ();

    fn begin(
        &self,
        _: &mut (),
        _: &mut C,
        _active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp {
        if child_count == 0 {
            ControlOp::Failure
        } else {
            ControlOp::RunChild(0)
        }
    }

    fn child_succeeded(
        &self,
        _: &mut (),
        _: &mut C,
        _completed_child_index: usize,
        _child_count: usize,
    ) -> ControlOp {
        ControlOp::Success
    }

    fn child_failed(
        &self,
        _: &mut (),
        _: &mut C,
        completed_child_index: usize,
        child_count: usize,
    ) -> ControlOp {
        if completed_child_index + 1 < child_count {
            ControlOp::RunChild(completed_child_index + 1)
        } else {
            ControlOp::Failure
        }
    }
}
