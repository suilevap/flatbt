use core::marker::PhantomData;

use super::read::ReadFn;
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, EntryMode, NodeResult};

/// Stateless callable. Captures hold configuration; context holds mutable data.
pub struct Leaf<F>(F);

/// Wraps a callable. Context effects are immediate and survive failure.
/// Implement [`BtNode`] directly for invocation-local state.
///
/// The callable is checked where the tree runs, not here, so a closure stays
/// open to inference. That is what lets one closure serve a context borrowed
/// for the update, whose lifetimes the tree only fixes when it is used.
///
/// A leaf that returns `Running` has to say what the agent is doing, which is
/// usually a sign it wants writing as an action instead -- a leaf is re-entered
/// on every resume and has no state to make progress with.
pub fn leaf<F>(f: F) -> Leaf<F> {
    Leaf(f)
}

impl<C, A, P, F: Fn(&mut C) -> NodeResult<A>> BtNode<C, A, P> for Leaf<F> {
    type State = ();

    #[inline(always)]
    fn update(&self, _: &mut (), ctx: &mut C, _: P, _: EntryMode) -> NodeResult<A> {
        (self.0)(ctx)
    }
}

/// Predicate over shared context.
pub struct Check<F>(F);

/// Returns Success for true, Failure for false. Checked where the tree runs,
/// like [`leaf`].
///
/// A predicate never occupies the agent, so it never names the act type.
pub fn check<F>(predicate: F) -> Check<F> {
    Check(predicate)
}

impl<C, A, P, F: Fn(&C) -> bool> BtNode<C, A, P> for Check<F> {
    type State = ();

    #[inline(always)]
    fn update(&self, _: &mut (), ctx: &mut C, _: P, _: EntryMode) -> NodeResult<A> {
        if (self.0)(ctx) {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }
}

/// A child that runs only while a predicate holds.
pub struct Guarded<F, N, M> {
    predicate: F,
    child: N,
    reads: PhantomData<fn() -> M>,
}

/// Runs `child` while `predicate` holds; fails without entering it otherwise.
///
/// Checked on every update, Resume included, so it ends a running child the
/// moment its condition stops holding. A [`check`] before it in a [`seq`] is
/// asked only on entry: while the child runs, the sequence goes straight back
/// to it.
///
/// ```
/// use flatbt::{BtState, EntryMode, NodeResult, guard, leaf, update};
///
/// let tree = guard(|ammo: &u32| *ammo > 0, leaf(|_: &mut u32| NodeResult::RUNNING));
/// let mut state = BtState::new(&tree);
/// let mut ammo = 1;
/// assert!(update(&tree, &mut state, &mut ammo, EntryMode::Resume).is_running());
/// ammo = 0;
/// assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Failure);
/// ```
///
/// The predicate is `Fn(&C) -> bool`, or `Fn(&C, P) -> bool` to also read the
/// parameters it forwards to `child` -- a target bound with `.with(target)`,
/// asked about on every update. See [`ReadFn`].
///
/// [`seq`]: crate::seq
pub fn guard<F, N, M>(predicate: F, child: N) -> Guarded<F, N, M> {
    Guarded {
        predicate,
        child,
        reads: PhantomData,
    }
}

impl<C, A, P: ParamValue, F, N, S, M> BtNode<C, A, P> for Guarded<F, N, M>
where
    F: ReadFn<C, P, bool, M>,
    N: for<'a> BtNode<C, A, <P::Shape as ParamShape>::Value<'a>, State = S>,
    S: Default + Send + 'static,
{
    type State = S;

    #[inline(always)]
    fn update(&self, state: &mut S, ctx: &mut C, params: P, mode: EntryMode) -> NodeResult<A> {
        let mut params = params.into_value();
        if self.predicate.call(ctx, P::Shape::reborrow(&mut params)) {
            self.child.update(state, ctx, params, mode)
        } else {
            NodeResult::Failure
        }
    }
}
