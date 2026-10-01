use super::{BtClock, elapsed};
use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult};

/// A child that cannot start again until a span of time has passed.
pub struct Cooldown<Span, Child> {
    span: Span,
    child: Child,
    after_success: bool,
}

/// Fails without starting `child` until `span` has passed since the child last
/// started. Measured from each try, successful or not: `cooldown(2s, dash)`
/// dashes at most once every two seconds.
///
/// The time is kept in the node's memory, so it outlives the run and is the
/// agent's own. A child that starts and is rejected in the same update, such
/// as a failed candidate under `Evaluate`, still counts as a try. A running
/// child is never interrupted.
pub fn cooldown<Span, Child>(span: Span, child: Child) -> Cooldown<Span, Child> {
    Cooldown {
        span,
        child,
        after_success: false,
    }
}

/// Like [`cooldown`], but measured from the child's last success: failed
/// attempts can be retried at once. `success_cooldown(10s, heal)` heals at
/// most once every ten seconds, however many heals were interrupted.
pub fn success_cooldown<Span, Child>(span: Span, child: Child) -> Cooldown<Span, Child> {
    Cooldown {
        span,
        child,
        after_success: true,
    }
}

/// Whether the child has started in this run, and its state.
#[derive(Default)]
pub struct CooldownState<ChildState> {
    child: ChildState,
    started: bool,
}

/// When the cooldown last began, and the child's memory.
pub struct CooldownMemory<Instant, ChildMemory> {
    child: ChildMemory,
    last: Option<Instant>,
}

// Derived `Default` would require `Instant: Default`.
impl<Instant, ChildMemory: Default> Default for CooldownMemory<Instant, ChildMemory> {
    fn default() -> Self {
        Self {
            child: ChildMemory::default(),
            last: None,
        }
    }
}

impl<Context, Act, Params, Child, ChildState, ChildMemory> BtNode<Context, Act, Params>
    for Cooldown<Context::Duration, Child>
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
    type State = CooldownState<ChildState>;
    type Memory = CooldownMemory<Context::Instant, ChildMemory>;
    const NODES: usize =
        1 + <Child as BtNode<Context, Act, <Params::Shape as ParamShape>::Value<'static>>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        if !state.started {
            if let Some(last) = memory.last
                && !elapsed(ctx, last, self.span)
            {
                entry.record("cooling", || true);
                return NodeResult::Failure;
            }
            state.started = true;
            if !self.after_success {
                memory.last = Some(ctx.now());
            }
        }
        let result = entry.run(
            1,
            &self.child,
            &mut state.child,
            &mut memory.child,
            ctx,
            params.into_value(),
        );
        if self.after_success && matches!(result, NodeResult::Success) {
            memory.last = Some(ctx.now());
        }
        result
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &Self::Memory,
        inspector: &mut dyn Inspector,
    ) {
        let kind = if self.after_success {
            "success_cooldown"
        } else {
            "cooldown"
        };
        inspector.node(NodeInfo::new(kind, state.is_some()), |inspector| {
            inspector.field("span", &self.span);
            if let Some(last) = &memory.last {
                inspector.field("last", last);
            }
            BtNode::<Context, Act, <Params::Shape as ParamShape>::Value<'_>>::inspect(
                &self.child,
                state.map(|state| &state.child),
                &memory.child,
                inspector,
            );
        });
    }
}
