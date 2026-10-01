use super::{BtClock, elapsed};
use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult};

/// A child given a span of time to finish.
pub struct Timeout<D, N> {
    span: D,
    child: N,
}

/// Runs `child`, and fails once `span` has passed since it started, ending
/// the child as any preempted node ends: its state is dropped, so an action
/// is cancelled.
///
/// Checked on every update, before the child. The time runs from the update
/// this node starts on and is kept under `Evaluate`, as the child's run is.
pub fn timeout<D, N>(span: D, child: N) -> Timeout<D, N> {
    Timeout { span, child }
}

/// When the child started, and its state.
pub struct TimeoutState<I, S> {
    child: S,
    started: Option<I>,
}

// Derived `Default` would require `I: Default`.
impl<I, S: Default> Default for TimeoutState<I, S> {
    fn default() -> Self {
        Self {
            child: S::default(),
            started: None,
        }
    }
}

impl<C, A, P, N, S, Mem> BtNode<C, A, P> for Timeout<C::Duration, N>
where
    C: BtClock,
    P: ParamValue,
    N: for<'a> BtNode<C, A, <P::Shape as ParamShape>::Value<'a>, State = S, Memory = Mem>,
    S: Default + Send + 'static,
    Mem: Default + Send + 'static,
{
    type State = TimeoutState<C::Instant, S>;
    type Memory = Mem;
    const NODES: usize = 1 + <N as BtNode<C, A, <P::Shape as ParamShape>::Value<'static>>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Mem,
        ctx: &mut C,
        params: P,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
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

    fn inspect(&self, state: Option<&Self::State>, memory: &Mem, inspector: &mut dyn Inspector) {
        inspector.node(NodeInfo::new("timeout", state.is_some()), |inspector| {
            inspector.field("span", &self.span);
            if let Some(started) = state.and_then(|state| state.started.as_ref()) {
                inspector.field("started", started);
            }
            BtNode::<C, A, <P::Shape as ParamShape>::Value<'_>>::inspect(
                &self.child,
                state.map(|state| &state.child),
                memory,
                inspector,
            );
        });
    }
}
