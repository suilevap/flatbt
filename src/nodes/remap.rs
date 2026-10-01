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
    fn result<Act>(self) -> NodeResult<Act> {
        match self {
            Self::Success => NodeResult::Success,
            Self::Failure => NodeResult::Failure,
        }
    }
}

/// A child whose terminal results are mapped; `Running` passes through.
pub struct Remap<Child> {
    child: Child,
    success: Outcome,
    failure: Outcome,
}

/// Swaps Success and Failure; `Running` and its act pass through.
pub fn invert<Child>(child: Child) -> Remap<Child> {
    Remap {
        child,
        success: Outcome::Failure,
        failure: Outcome::Success,
    }
}

/// Succeeds whenever `child` ends; `Running` passes through.
pub fn force_success<Child>(child: Child) -> Remap<Child> {
    Remap {
        child,
        success: Outcome::Success,
        failure: Outcome::Success,
    }
}

/// Fails whenever `child` ends; `Running` passes through.
pub fn force_failure<Child>(child: Child) -> Remap<Child> {
    Remap {
        child,
        success: Outcome::Failure,
        failure: Outcome::Failure,
    }
}

impl<Context, Act, Params, Child: BtNode<Context, Act, Params>> BtNode<Context, Act, Params>
    for Remap<Child>
{
    type State = Child::State;
    type Memory = Child::Memory;
    const NODES: usize = 1 + Child::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Child::State,
        memory: &mut Child::Memory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        match entry.run(1, &self.child, state, memory, ctx, params) {
            running @ NodeResult::Running(_) => running,
            NodeResult::Success => self.success.result(),
            NodeResult::Failure => self.failure.result(),
        }
    }

    fn inspect(
        &self,
        state: Option<&Child::State>,
        memory: &Child::Memory,
        inspector: &mut dyn Inspector,
    ) {
        let kind = match (self.success, self.failure) {
            (Outcome::Failure, Outcome::Success) => "invert",
            (Outcome::Success, _) => "force_success",
            (Outcome::Failure, _) => "force_failure",
        };
        inspector.node(NodeInfo::new(kind, state.is_some()), |inspector| {
            self.child.inspect(state, memory, inspector);
        });
    }
}
