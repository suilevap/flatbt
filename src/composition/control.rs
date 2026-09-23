use crate::params::{ParamShape, ParamValue};
use crate::{BtChildren, BtNode, EntryMode, NodeResult};

/// Next step requested by a policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlOp {
    RunChild(usize),
    Success,
    Failure,
}

impl ControlOp {
    /// Reports a diagnostic and terminates the control with Failure. See
    /// `set_error_handler`.
    pub fn error(message: impl core::fmt::Display) -> Self {
        crate::log_error(message);
        Self::Failure
    }
}

/// Policy state plus child state, whose variant encodes selection.
#[derive(Default)]
pub struct ControlState<S, ChildrenState> {
    // Drop descendants before policy state.
    children: ChildrenState,
    inner: S,
}

/// Static control-flow policy with per-invocation state.
/// Child indices must be below `child_count`. Policies must terminate;
/// execution has no iteration budget.
pub trait BtControl<C> {
    type State: Default + Send + 'static;

    /// Selects a child on fresh entry or Evaluate. Resume skips this callback.
    /// `active_child_index` reports saved selection; the framework owns it.
    fn begin(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp;

    /// Called after the successful child invocation ends.
    fn child_succeeded(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        completed_child_index: usize,
        child_count: usize,
    ) -> ControlOp;

    /// Called after the failed child invocation ends.
    fn child_failed(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        completed_child_index: usize,
        child_count: usize,
    ) -> ControlOp;
}

/// Policy with statically typed children.
pub struct ControlNode<P, Children> {
    policy: P,
    children: Children,
}

/// Combines a policy and children with static dispatch.
pub fn control<P, Children>(policy: P, children: Children) -> ControlNode<P, Children> {
    ControlNode { policy, children }
}

impl<C, A, Params: ParamValue, P: BtControl<C>, Children, S> BtNode<C, A, Params>
    for ControlNode<P, Children>
where
    Children: for<'a> BtChildren<C, A, <Params::Shape as ParamShape>::Value<'a>, State = S>,
    S: Default + Send + 'static,
{
    type State = ControlState<P::State, S>;

    // A tree is one type. Inlining every level into the root's update lets the
    // compiler see the whole path; LLVM's own heuristics stop a few levels in.
    #[inline(always)]
    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: Params,
        mode: EntryMode,
    ) -> NodeResult<A> {
        let mut params = params.into_value();
        let active_child_index = self.children.active_child_index(&state.children);
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
                    match self.children.run_child(
                        &mut state.children,
                        child_index,
                        ctx,
                        Params::Shape::reborrow(&mut params),
                        mode,
                    ) {
                        // The act comes from whichever child actually ran.
                        running @ NodeResult::Running(_) => return running,
                        NodeResult::Success => self.policy.child_succeeded(
                            &mut state.inner,
                            ctx,
                            child_index,
                            Children::LEN,
                        ),
                        NodeResult::Failure => self.policy.child_failed(
                            &mut state.inner,
                            ctx,
                            child_index,
                            Children::LEN,
                        ),
                    }
                }
            };
        }
    }
}

/// Runs children in order; fails on first Failure. Evaluate preserves the active
/// child without replaying completed children.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sequence;

/// Runs a sequence. Empty sequences succeed.
pub fn seq<Children>(children: Children) -> ControlNode<Sequence, Children> {
    control(Sequence, children)
}

impl<C> BtControl<C> for Sequence {
    type State = ();

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
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

/// Tries children in order; stops on Success or Running.
/// Evaluate scans from child zero; Resume follows the saved child.
#[derive(Clone, Copy, Debug, Default)]
pub struct Selector;

/// Runs a selector. Empty selectors fail.
pub fn select<Children>(children: Children) -> ControlNode<Selector, Children> {
    control(Selector, children)
}

impl<C> BtControl<C> for Selector {
    type State = ();

    #[inline(always)]
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

    #[inline(always)]
    fn child_succeeded(
        &self,
        _: &mut (),
        _: &mut C,
        _completed_child_index: usize,
        _child_count: usize,
    ) -> ControlOp {
        ControlOp::Success
    }

    #[inline(always)]
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
