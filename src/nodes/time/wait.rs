use core::marker::PhantomData;

use super::{BtClock, elapsed};
use crate::inspect::{Inspector, NodeInfo};
use crate::params::ParamValue;
use crate::{BtNode, Entry, NodeResult, ReadFn};

/// An act reported for a span of time.
pub struct ActionWait<D, G, M> {
    span: D,
    act: G,
    reads: PhantomData<fn() -> M>,
}

/// Reports `act` until `span` has passed since this node started, then
/// succeeds. A zero span succeeds at once.
///
/// `act` is `Fn(&C)` or `Fn(&C, P)`, as for
/// [`action_while`](crate::action_while), and is asked on every update.
///
/// ```
/// use flatbt::prelude::*;
///
/// struct Game { turn: u32 }
/// impl BtClock for Game {
///     type Instant = u32;
///     type Duration = u32;
///     fn now(&self) -> u32 { self.turn }
/// }
///
/// let tree = action_wait(2, |_: &Game| "aim");
/// let mut state = BtState::new(&tree);
/// let mut game = Game { turn: 5 };
/// assert_eq!(update(&tree, &mut state, &mut game, EntryMode::Resume), NodeResult::Running("aim"));
/// game.turn = 7;
/// assert_eq!(update(&tree, &mut state, &mut game, EntryMode::Resume), NodeResult::Success);
/// ```
pub fn action_wait<D, G, M>(span: D, act: G) -> ActionWait<D, G, M> {
    ActionWait {
        span,
        act,
        reads: PhantomData,
    }
}

impl<C, A, P, G, M> BtNode<C, A, P> for ActionWait<C::Duration, G, M>
where
    C: BtClock,
    P: ParamValue,
    G: ReadFn<C, P, A, M>,
{
    /// When this node started.
    type State = Option<C::Instant>;
    type Memory = ();

    #[inline]
    fn update(
        &self,
        started: &mut Option<C::Instant>,
        _: &mut (),
        ctx: &mut C,
        params: P,
        _: Entry<'_>,
    ) -> NodeResult<A> {
        let started = *started.get_or_insert_with(|| ctx.now());
        if elapsed(ctx, started, self.span) {
            NodeResult::Success
        } else {
            NodeResult::Running(self.act.call(ctx, params.into_value()))
        }
    }

    fn inspect(&self, started: Option<&Option<C::Instant>>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("action_wait", started.is_some()).with_fn_name::<G>();
        inspector.node(node, |inspector| {
            inspector.field("span", &self.span);
            if let Some(Some(started)) = started {
                inspector.field("started", started);
            }
        });
    }
}
