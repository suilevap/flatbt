use core::marker::PhantomData;

use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, EntryMode, NodeResult, ReadFn};

/// A child resumed as Evaluate while a condition holds.
pub struct ReevaluateWhen<F, N, M> {
    condition: F,
    child: N,
    reads: PhantomData<fn() -> M>,
}

/// Passes `Resume` down to `child` as `Evaluate` on updates where
/// `condition` holds, so that subtree reconsiders its choices even when the
/// rest of the tree only resumes; otherwise passes the mode through.
///
/// For something that should make the agent reconsider, such as an alarm
/// changing: `reevaluate_when(|bb: &Guard| bb.alarm_changed, combat)`. The
/// condition is `Fn(&C) -> bool` or `Fn(&C, P) -> bool`, see [`ReadFn`].
/// Never converts unconditionally: a subtree evaluated on every update is what
/// `Evaluate` at the root already gives.
pub fn reevaluate_when<F, N, M>(condition: F, child: N) -> ReevaluateWhen<F, N, M> {
    ReevaluateWhen {
        condition,
        child,
        reads: PhantomData,
    }
}

impl<C, A, P: ParamValue, F, N, S, M> BtNode<C, A, P> for ReevaluateWhen<F, N, M>
where
    F: ReadFn<C, P, bool, M>,
    N: for<'a> BtNode<C, A, <P::Shape as ParamShape>::Value<'a>, State = S>,
    S: Default + Send + 'static,
{
    type State = S;

    #[inline]
    fn update(&self, state: &mut S, ctx: &mut C, params: P, mode: EntryMode) -> NodeResult<A> {
        let mut params = params.into_value();
        let mode = if mode == EntryMode::Resume
            && self.condition.call(ctx, P::Shape::reborrow(&mut params))
        {
            EntryMode::Evaluate
        } else {
            mode
        };
        self.child.update(state, ctx, params, mode)
    }
}
