use crate::inspect::{Inspector, NodeInfo};
use crate::{BtNode, Entry, NodeResult};

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
    const NODES: usize = 1 + N::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut N::State,
        ctx: &mut C,
        params: P,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        let entry = entry.child(1);
        let result = self.child.update(state, ctx, params, entry);
        entry.finish(&result);
        match result {
            running @ NodeResult::Running(_) => running,
            NodeResult::Success => self.success.result(),
            NodeResult::Failure => self.failure.result(),
        }
    }

    fn inspect(&self, state: Option<&N::State>, inspector: &mut dyn Inspector) {
        let kind = match (self.success, self.failure) {
            (Outcome::Failure, Outcome::Success) => "invert",
            (Outcome::Success, _) => "force_success",
            (Outcome::Failure, _) => "force_failure",
        };
        inspector.node(NodeInfo::new(kind, state.is_some()), |inspector| {
            self.child.inspect(state, inspector);
        });
    }
}
