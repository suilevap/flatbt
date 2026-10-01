use super::{BtClock, elapsed};
use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, EntryMode, NodeResult};

/// A child resumed as Evaluate once per span of time.
pub struct ReevaluateEvery<Span, Child> {
    span: Span,
    child: Child,
}

/// Passes `Resume` down to `child` as `Evaluate` once `span` has passed since
/// the subtree was last evaluated, so it reconsiders its choices at that rate
/// while the rest of the tree only resumes. A time-based
/// [`reevaluate_when`](crate::reevaluate_when).
///
/// The time runs from when this node started, or was last evaluated by
/// either its parent or itself.
pub fn reevaluate_every<Span, Child>(span: Span, child: Child) -> ReevaluateEvery<Span, Child> {
    ReevaluateEvery { span, child }
}

/// When the child was last evaluated, and its state.
pub struct ReevaluateEveryState<Instant, ChildState> {
    child: ChildState,
    evaluated: Option<Instant>,
}

// Derived `Default` would require `Instant: Default`.
impl<Instant, ChildState: Default> Default for ReevaluateEveryState<Instant, ChildState> {
    fn default() -> Self {
        Self {
            child: ChildState::default(),
            evaluated: None,
        }
    }
}

impl<Context, Act, Params, Child, ChildState, ChildMemory> BtNode<Context, Act, Params>
    for ReevaluateEvery<Context::Duration, Child>
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
    type State = ReevaluateEveryState<Context::Instant, ChildState>;
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
        let due = match (entry.mode(), state.evaluated) {
            (EntryMode::Resume, Some(evaluated)) => elapsed(ctx, evaluated, self.span),
            _ => true,
        };
        let entry = if due {
            state.evaluated = Some(ctx.now());
            if entry.mode() == EntryMode::Resume {
                entry.record("due", || true);
            }
            entry.with_mode(EntryMode::Evaluate)
        } else {
            entry
        };
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
        inspector.node(
            NodeInfo::new("reevaluate_every", state.is_some()),
            |inspector| {
                inspector.field("span", &self.span);
                if let Some(evaluated) = state.and_then(|state| state.evaluated.as_ref()) {
                    inspector.field("evaluated", evaluated);
                }
                BtNode::<Context, Act, <Params::Shape as ParamShape>::Value<'_>>::inspect(
                    &self.child,
                    state.map(|state| &state.child),
                    memory,
                    inspector,
                );
            },
        );
    }
}
