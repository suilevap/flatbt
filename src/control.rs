use crate::{BtChildren, BtNode, NodeResult};

/// The next step requested by a control policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlOp {
    RunChild(usize),
    Success,
    Failure,
}

/// A statically dispatched control-flow policy, separate from node execution.
///
/// State is local to one synchronous invocation in M0. Child indices must be
/// below `child_count`. Policies must eventually terminate: there is currently
/// no execution budget to stop a policy that repeatedly requests a child.
pub trait BtControl<C> {
    type State: Default;

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
    fn update(&self, ctx: &mut C) -> NodeResult {
        let mut state = P::State::default();
        let mut op = self.policy.begin(&mut state, ctx, Children::LEN);
        loop {
            op = match op {
                ControlOp::Success => return NodeResult::Success,
                ControlOp::Failure => return NodeResult::Failure,
                ControlOp::RunChild(index) => {
                    assert!(
                        index < Children::LEN,
                        "control policy returned invalid child index {index} for {} children",
                        Children::LEN
                    );
                    match self.children.run_child(index, ctx) {
                        NodeResult::Success => {
                            self.policy
                                .child_succeeded(&mut state, ctx, index, Children::LEN)
                        }
                        NodeResult::Failure => {
                            self.policy
                                .child_failed(&mut state, ctx, index, Children::LEN)
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
