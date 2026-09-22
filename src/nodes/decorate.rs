use core::marker::PhantomData;

use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, EntryMode, NodeResult};

/// A child restarted while a condition holds.
pub struct RepeatWhile<F, N> {
    condition: F,
    child: N,
}

/// Keeps the agent busy with `child` while `condition` holds; succeeds once it
/// does not.
///
/// A goal rather than a requirement, so it reads as a prerequisite for what
/// follows: `seq((repeat_while(far, approach), interact))` succeeds at once for
/// an agent already close. [`guard`](crate::guard) is the requirement: a false
/// condition fails it.
///
/// `condition` is asked on entry, on every update, `Resume` included, and
/// whenever the child completes:
///
/// - false: Success. A running child is dropped, which cancels it.
/// - the child succeeds after running: it restarts in the same update.
/// - the child fails: Failure.
/// - the child completes without returning `Running` since it started: Failure.
///   A loop around an instant child would spin with no act to report; a restart
///   that does so also reports a diagnostic.
///
/// Parameters are forwarded to the child.
///
/// ```
/// use flatbt::prelude::*;
///
/// // Walks until it arrives; the next node in a sequence would then run.
/// let tree = repeat_while(
///     |pos: &u32| *pos < 2,
///     action_while(|_: &u32| true, |pos: &u32| *pos + 1),
/// );
/// let mut state = BtState::new(&tree);
/// let mut pos = 0;
/// assert_eq!(update(&tree, &mut state, &mut pos, EntryMode::Resume), NodeResult::Running(1));
/// pos = 2;
/// assert_eq!(update(&tree, &mut state, &mut pos, EntryMode::Resume), NodeResult::Success);
/// ```
pub fn repeat_while<F, N>(condition: F, child: N) -> RepeatWhile<F, N> {
    RepeatWhile { condition, child }
}

/// Child state, and whether it has returned `Running` since it started.
#[derive(Default)]
pub struct RepeatWhileState<S> {
    child: S,
    ran: bool,
}

impl<C, A, P: ParamValue, F, N, S> BtNode<C, A, P> for RepeatWhile<F, N>
where
    F: Fn(&C) -> bool,
    N: for<'a> BtNode<C, A, <P::Shape as ParamShape>::Value<'a>, State = S>,
    S: Default + Send + 'static,
{
    type State = RepeatWhileState<S>;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: P,
        mut mode: EntryMode,
    ) -> NodeResult<A> {
        if !(self.condition)(ctx) {
            return NodeResult::Success;
        }
        let mut params = params.into_value();
        let mut restarted = false;
        loop {
            let result =
                self.child
                    .update(&mut state.child, ctx, P::Shape::reborrow(&mut params), mode);
            if result.is_running() {
                state.ran = true;
                return result;
            }
            let ran = core::mem::take(&mut state.ran);
            state.child = S::default();
            if !(self.condition)(ctx) {
                return NodeResult::Success;
            }
            match result {
                NodeResult::Success if ran => {}
                NodeResult::Success if restarted => {
                    return NodeResult::error(
                        "repeat_while: restarted child completed without running",
                    );
                }
                _ => return NodeResult::Failure,
            }
            restarted = true;
            mode = EntryMode::Evaluate;
        }
    }
}

/// A child whose act is converted.
pub struct MapAct<F, N, B> {
    map: F,
    child: N,
    act: PhantomData<fn() -> B>,
}

/// Runs `child`, deciding `B`, in a tree deciding `A`: its act passes through
/// `map`, and its results are otherwise unchanged.
///
/// Lets a subtree keep its own act type and be reused under a larger one; an
/// enum variant wrapping the smaller act is the usual `map`.
///
/// ```
/// use flatbt::prelude::*;
///
/// #[derive(Debug, PartialEq)]
/// enum Walk { To(u32) }
/// #[derive(Debug, PartialEq)]
/// enum Act { Walk(Walk) }
///
/// let tree = map_act(Act::Walk, leaf(|_: &mut ()| NodeResult::Running(Walk::To(3))));
/// let mut state = BtState::new(&tree);
/// let doing = update(&tree, &mut state, &mut (), EntryMode::Evaluate).act();
/// assert_eq!(doing, Some(Act::Walk(Walk::To(3))));
/// ```
pub fn map_act<F, N, B>(map: F, child: N) -> MapAct<F, N, B> {
    MapAct {
        map,
        child,
        act: PhantomData,
    }
}

impl<C, A, B, P, F: Fn(B) -> A, N: BtNode<C, B, P>> BtNode<C, A, P> for MapAct<F, N, B> {
    type State = N::State;

    fn update(
        &self,
        state: &mut N::State,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult<A> {
        match self.child.update(state, ctx, params, mode) {
            NodeResult::Running(act) => NodeResult::Running((self.map)(act)),
            NodeResult::Success => NodeResult::Success,
            NodeResult::Failure => NodeResult::Failure,
        }
    }
}
