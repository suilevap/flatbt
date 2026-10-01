use super::{BtClock, elapsed};
use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult};

/// A child given a span of time to finish.
pub struct Timeout<Span, Child> {
    span: Span,
    child: Child,
}

/// Runs `child`, and fails once `span` has passed since it started, ending
/// the child as any preempted node ends: its state is dropped, so an action
/// is cancelled.
///
/// Checked on every update, before the child. The time runs from the update
/// this node starts on and is kept under `Evaluate`, as the child's run is.
pub fn timeout<Span, Child>(span: Span, child: Child) -> Timeout<Span, Child> {
    Timeout { span, child }
}

/// When the child started, and its state.
pub struct TimeoutState<Instant, ChildState> {
    child: ChildState,
    started: Option<Instant>,
}

// Derived `Default` would require `Instant: Default`.
impl<Instant, ChildState: Default> Default for TimeoutState<Instant, ChildState> {
    fn default() -> Self {
        Self {
            child: ChildState::default(),
            started: None,
        }
    }
}

impl<Context, Act, Params, Child, ChildState, ChildMemory> BtNode<Context, Act, Params>
    for Timeout<Context::Duration, Child>
where
    Context: BtClock,
    Params: ParamValue,
    Child: for<'a> BtNode<
            Context,
            Act,
            <Params::Shape as ParamShape>::Value<'a>,
            State = ChildState,
            Memory = ChildMemory,
        >,
    ChildState: Default + Send + 'static,
    ChildMemory: Default + Send + 'static,
{
    type State = TimeoutState<Context::Instant, ChildState>;
    type Memory = ChildMemory;
    const NODES: usize =
        1 + <Child as BtNode<Context, Act, <Params::Shape as ParamShape>::Value<'static>>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut ChildMemory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let started = *state.started.get_or_insert_with(|| ctx.now());
        if elapsed(ctx, started, self.span) {
            entry.record("timed_out", || true);
            return NodeResult::Failure;
        }
        entry.run(
            1,
            &self.child,
            &mut state.child,
            memory,
            ctx,
            params.into_value(),
        )
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &ChildMemory,
        inspector: &mut dyn Inspector,
    ) {
        inspector.node(NodeInfo::new("timeout", state.is_some()), |inspector| {
            inspector.field("span", &self.span);
            if let Some(started) = state.and_then(|state| state.started.as_ref()) {
                inspector.field("started", started);
            }
            BtNode::<Context, Act, <Params::Shape as ParamShape>::Value<'_>>::inspect(
                &self.child,
                state.map(|state| &state.child),
                memory,
                inspector,
            );
        });
    }
}
