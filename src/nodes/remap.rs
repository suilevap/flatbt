use crate::{BtNode, EntryMode, NodeResult};

/// A terminal result a child's result is mapped to.
#[derive(Clone, Copy)]
enum Outcome {
    Success,
    Failure,
}

impl Outcome {
    #[inline]
    fn result<A>(self) -> NodeResult<A> {
        match self {
            Self::Success => NodeResult::Success,
            Self::Failure => NodeResult::Failure,
        }
    }
}

/// A child whose terminal results are mapped; `Running` passes through.
pub struct Remap<N> {
    child: N,
    success: Outcome,
    failure: Outcome,
}

/// Swaps Success and Failure; `Running` and its act pass through.
pub fn invert<N>(child: N) -> Remap<N> {
    Remap {
        child,
        success: Outcome::Failure,
        failure: Outcome::Success,
    }
}

/// Succeeds whenever `child` ends; `Running` passes through.
pub fn force_success<N>(child: N) -> Remap<N> {
    Remap {
        child,
        success: Outcome::Success,
        failure: Outcome::Success,
    }
}

/// Fails whenever `child` ends; `Running` passes through.
pub fn force_failure<N>(child: N) -> Remap<N> {
    Remap {
        child,
        success: Outcome::Failure,
        failure: Outcome::Failure,
    }
}

impl<C, A, P, N: BtNode<C, A, P>> BtNode<C, A, P> for Remap<N> {
    type State = N::State;

    #[inline]
    fn update(
        &self,
        state: &mut N::State,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult<A> {
        match self.child.update(state, ctx, params, mode) {
            running @ NodeResult::Running(_) => running,
            NodeResult::Success => self.success.result(),
            NodeResult::Failure => self.failure.result(),
        }
    }
}
