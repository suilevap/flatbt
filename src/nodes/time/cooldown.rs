use super::{BtClock, elapsed};
use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult};

/// A child that cannot start again until a span of time has passed.
pub struct Cooldown<D, N> {
    span: D,
    child: N,
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
pub fn cooldown<D, N>(span: D, child: N) -> Cooldown<D, N> {
    Cooldown {
        span,
        child,
        after_success: false,
    }
}

/// Like [`cooldown`], but measured from the child's last success: failed
/// attempts can be retried at once. `success_cooldown(10s, heal)` heals at
/// most once every ten seconds, however many heals were interrupted.
pub fn success_cooldown<D, N>(span: D, child: N) -> Cooldown<D, N> {
    Cooldown {
        span,
        child,
        after_success: true,
    }
}

/// Whether the child has started in this run, and its state.
#[derive(Default)]
pub struct CooldownState<S> {
    child: S,
    started: bool,
}

/// When the cooldown last began, and the child's memory.
pub struct CooldownMemory<I, M> {
    child: M,
    last: Option<I>,
}

// Derived `Default` would require `I: Default`.
impl<I, M: Default> Default for CooldownMemory<I, M> {
    fn default() -> Self {
        Self {
            child: M::default(),
            last: None,
        }
    }
}

impl<C, A, P, N, S, Mem> BtNode<C, A, P> for Cooldown<C::Duration, N>
where
    C: BtClock,
    P: ParamValue,
    N: for<'a> BtNode<C, A, <P::Shape as ParamShape>::Value<'a>, State = S, Memory = Mem>,
    S: Default + Send + 'static,
    Mem: Default + Send + 'static,
{
    type State = CooldownState<S>;
    type Memory = CooldownMemory<C::Instant, Mem>;
    const NODES: usize = 1 + <N as BtNode<C, A, <P::Shape as ParamShape>::Value<'static>>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut C,
        params: P,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
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
            BtNode::<C, A, <P::Shape as ParamShape>::Value<'_>>::inspect(
                &self.child,
                state.map(|state| &state.child),
                &memory.child,
                inspector,
            );
        });
    }
}
