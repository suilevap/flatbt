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
    /// Logs to stderr and terminates the control with Failure.
    pub fn error(message: impl std::fmt::Display) -> Self {
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

    /// Called instead of [`child_failed`](BtControl::child_failed) when the
    /// child that failed was the resumed continuation.
    ///
    /// Resume skips [`begin`](BtControl::begin), so no child before the active
    /// one was consulted this update. While the continuation holds that is the
    /// point. Once it fails there is nothing left to preserve, and whatever the
    /// policy decides next it decides on information it never gathered -- so a
    /// policy whose order means priority answers here instead.
    ///
    /// The framework will not run the failed child again in the same update.
    /// Defaults to `child_failed`.
    fn continuation_failed(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        completed_child_index: usize,
        child_count: usize,
    ) -> ControlOp {
        self.child_failed(state, ctx, completed_child_index, child_count)
    }
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

impl<C, A: ParamValue, P: BtControl<C>, Children, S> BtNode<C, A> for ControlNode<P, Children>
where
    Children: for<'a> BtChildren<C, <A::Shape as ParamShape>::Value<'a>, State = S>,
    S: Default + Send + 'static,
{
    type State = ControlState<P::State, S>;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: A,
        mode: EntryMode,
    ) -> NodeResult {
        let mut params = params.into_value();
        let active_child_index = self.children.active_child_index(&state.children);
        // The child this update resumes, if any: the only one entered with a
        // saved continuation, and the only one the policy did not choose.
        let mut continuation = None;
        // Set once a continuation has failed, so the rescan does not rerun it.
        let mut failed = None;
        let mut op = match (mode, active_child_index) {
            (EntryMode::Resume, Some(child_index)) => {
                continuation = Some(child_index);
                ControlOp::RunChild(child_index)
            }
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
                    // A policy rescanning after its continuation failed reaches
                    // that child again. It already failed this update, and
                    // running it twice would repeat its effects.
                    if failed.is_some_and(|failed| failed == child_index) {
                        op = self.policy.child_failed(
                            &mut state.inner,
                            ctx,
                            child_index,
                            Children::LEN,
                        );
                        continue;
                    }
                    let resumed = continuation == Some(child_index);
                    continuation = None;
                    match self.children.run_child(
                        &mut state.children,
                        child_index,
                        ctx,
                        A::Shape::reborrow(&mut params),
                        mode,
                    ) {
                        NodeResult::Running => return NodeResult::Running,
                        NodeResult::Success => self.policy.child_succeeded(
                            &mut state.inner,
                            ctx,
                            child_index,
                            Children::LEN,
                        ),
                        NodeResult::Failure if resumed => {
                            failed = Some(child_index);
                            self.policy.continuation_failed(
                                &mut state.inner,
                                ctx,
                                child_index,
                                Children::LEN,
                            )
                        }
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

/// Tries children in order; stops on Success or Running.
/// Evaluate scans from child zero; Resume follows the saved child, and rescans
/// from zero if that child fails.
#[derive(Clone, Copy, Debug, Default)]
pub struct Selector;

/// Runs a selector. Empty selectors fail.
pub fn select<Children>(children: Children) -> ControlNode<Selector, Children> {
    control(Selector, children)
}

impl<C> BtControl<C> for Selector {
    type State = ();

    /// Order is priority, so a lost continuation means rescanning from the top:
    /// the children above the resumed one were skipped, not rejected, and one
    /// of them may have become available while it ran.
    fn continuation_failed(
        &self,
        _: &mut (),
        _: &mut C,
        _: usize,
        child_count: usize,
    ) -> ControlOp {
        if child_count == 0 {
            ControlOp::Failure
        } else {
            ControlOp::RunChild(0)
        }
    }

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
