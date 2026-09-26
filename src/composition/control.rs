use crate::inspect::{Inspector, NodeInfo, type_label};
use crate::params::{ParamShape, ParamValue};
use crate::{BtChildren, BtNode, Entry, EntryMode, NodeResult};

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
    #[cold]
    #[inline(never)]
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

    /// What debug views call this control, such as `seq`. Defaults to the
    /// type name.
    fn kind(&self) -> &'static str {
        type_label::<Self>()
    }

    /// Reports fields of this control to `inspector`, before its children:
    /// configuration, and policy state while it runs. `active_child_index` is
    /// the saved selection, as [`begin`](Self::begin) receives it. Reports
    /// nothing by default.
    fn inspect(
        &self,
        _state: Option<&Self::State>,
        _active_child_index: Option<usize>,
        _inspector: &mut dyn Inspector,
    ) {
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

impl<C, A, Params: ParamValue, P: BtControl<C>, Children> BtNode<C, A, Params>
    for ControlNode<P, Children>
where
    Children: BtChildren<C, A, Params::Shape>,
{
    type State = ControlState<P::State, Children::State>;
    const NODES: usize = 1 + Children::NODES;

    // A tree is one type. Inlining every level into the root's update lets the
    // compiler see the whole path; LLVM's own heuristics stop a few levels in.
    #[inline(always)]
    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        let mut params = params.into_value();
        let active_child_index = self.children.active_child_index(&state.children);
        let op = match (entry.mode(), active_child_index) {
            (EntryMode::Resume, Some(child_index)) => ControlOp::RunChild(child_index),
            _ => self
                .policy
                .begin(&mut state.inner, ctx, active_child_index, Children::LEN),
        };
        // No loop here: once this is inlined into its parent, a loop would let
        // the compiler hoist every descendant's address out of it and spill them.
        // `run_from` already follows children in order, which is all that
        // `Sequence` and `Selector` ask for.
        match self.run(state, op, ctx, &mut params, entry) {
            Ok(result) => result,
            Err(op) => self.run_rest(state, op, ctx, &mut params, entry),
        }
    }

    fn inspect(&self, state: Option<&Self::State>, inspector: &mut dyn Inspector) {
        let kind = BtControl::<C>::kind(&self.policy);
        inspector.node(NodeInfo::new(kind, state.is_some()), |inspector| {
            let children = state.map(|state| &state.children);
            let active = children.and_then(|children| {
                BtChildren::<C, A, Params::Shape>::active_child_index(&self.children, children)
            });
            let policy = state.map(|state| &state.inner);
            BtControl::<C>::inspect(&self.policy, policy, active, inspector);
            BtChildren::<C, A, Params::Shape>::inspect_children(
                &self.children,
                children,
                inspector,
            );
        });
    }
}

impl<P, Children> ControlNode<P, Children> {
    /// Runs children from `op` while the policy asks for them in order.
    /// `Err` holds a request for any other child.
    #[inline(always)]
    fn run<C, A, S: ParamShape>(
        &self,
        state: &mut ControlState<P::State, Children::State>,
        op: ControlOp,
        ctx: &mut C,
        params: &mut S::Value<'_>,
        entry: Entry<'_>,
    ) -> Result<NodeResult<A>, ControlOp>
    where
        P: BtControl<C>,
        Children: BtChildren<C, A, S>,
    {
        let child_index = match op {
            ControlOp::Success => return Ok(NodeResult::Success),
            ControlOp::Failure => return Ok(NodeResult::Failure),
            ControlOp::RunChild(child_index) => child_index,
        };
        if child_index >= Children::LEN {
            return Ok(NodeResult::error(format_args!(
                "control policy returned invalid child index {child_index} for {} children",
                Children::LEN,
            )));
        }
        let inner = &mut state.inner;
        let mut next = |ctx: &mut C, completed: usize, succeeded: bool| {
            if succeeded {
                self.policy
                    .child_succeeded(inner, ctx, completed, Children::LEN)
            } else {
                self.policy
                    .child_failed(inner, ctx, completed, Children::LEN)
            }
        };
        match self.children.run_from(
            &mut state.children,
            child_index,
            ctx,
            params,
            entry,
            &mut next,
        ) {
            Ok(running) => Ok(running),
            Err(ControlOp::Success) => Ok(NodeResult::Success),
            Err(ControlOp::Failure) => Ok(NodeResult::Failure),
            Err(op) => Err(op),
        }
    }

    /// Follows a policy that jumps between children out of order.
    #[inline(never)]
    fn run_rest<C, A, S: ParamShape>(
        &self,
        state: &mut ControlState<P::State, Children::State>,
        mut op: ControlOp,
        ctx: &mut C,
        params: &mut S::Value<'_>,
        entry: Entry<'_>,
    ) -> NodeResult<A>
    where
        P: BtControl<C>,
        Children: BtChildren<C, A, S>,
    {
        loop {
            match self.run(state, op, ctx, params, entry) {
                Ok(result) => return result,
                Err(next) => op = next,
            }
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

    fn kind(&self) -> &'static str {
        "seq"
    }

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

    fn kind(&self) -> &'static str {
        "select"
    }

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
