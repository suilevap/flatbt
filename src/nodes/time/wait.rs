use core::marker::PhantomData;

use super::{BtClock, elapsed};
use crate::inspect::{Inspector, NodeInfo};
use crate::params::ParamValue;
use crate::{BtNode, Entry, NodeResult, ReadFn};

/// An act reported for a span of time.
pub struct ActionWait<Span, MakeAct, Reads> {
    span: Span,
    act: MakeAct,
    reads: PhantomData<fn() -> Reads>,
}

/// Reports `act` until `span` has passed since this node started, then
/// succeeds. A zero span succeeds at once.
///
/// `act` is `Fn(&Context)` or `Fn(&Context, Params)`, as for
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
pub fn action_wait<Span, MakeAct, Reads>(
    span: Span,
    act: MakeAct,
) -> ActionWait<Span, MakeAct, Reads> {
    ActionWait {
        span,
        act,
        reads: PhantomData,
    }
}

impl<Context, Act, Params, MakeAct, Reads> BtNode<Context, Act, Params>
    for ActionWait<Context::Duration, MakeAct, Reads>
where
    Context: BtClock,
    Params: ParamValue,
    MakeAct: ReadFn<Context, Params, Act, Reads>,
{
    /// When this node started.
    type State = Option<Context::Instant>;
    type Memory = ();

    #[inline]
    fn update(
        &self,
        started: &mut Option<Context::Instant>,
        _: &mut (),
        ctx: &mut Context,
        params: Params,
        _: Entry<'_>,
    ) -> NodeResult<Act> {
        let started = *started.get_or_insert_with(|| ctx.now());
        if elapsed(ctx, started, self.span) {
            NodeResult::Success
        } else {
            NodeResult::Running(self.act.call(ctx, params.into_value()))
        }
    }

    fn inspect(
        &self,
        started: Option<&Option<Context::Instant>>,
        _: &(),
        inspector: &mut dyn Inspector,
    ) {
        let node = NodeInfo::new("action_wait", started.is_some()).with_fn_name::<MakeAct>();
        inspector.node(node, |inspector| {
            inspector.field("span", &self.span);
            if let Some(Some(started)) = started {
                inspector.field("started", started);
            }
        });
    }
}
