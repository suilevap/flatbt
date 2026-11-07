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
/// `act` is `Fn(&Blackboard)` or `Fn(&Blackboard, Params)`, as for
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

impl<Blackboard, Act, Params, MakeAct, Reads> BtNode<Blackboard, Act, Params>
    for ActionWait<Blackboard::Duration, MakeAct, Reads>
where
    Blackboard: BtClock,
    Params: ParamValue,
    MakeAct: ReadFn<Blackboard, Params, Act, Reads>,
{
    /// When this node started.
    type State = Option<Blackboard::Instant>;
    type Memory = ();

    #[inline]
    fn update(
        &self,
        started: &mut Option<Blackboard::Instant>,
        _: &mut (),
        bb: &mut Blackboard,
        params: Params,
        _: Entry<'_>,
    ) -> NodeResult<Act> {
        let started = *started.get_or_insert_with(|| bb.now());
        if elapsed(bb, started, self.span) {
            NodeResult::Success
        } else {
            NodeResult::Running(self.act.call(bb, params.into_value()))
        }
    }

    fn inspect(
        &self,
        started: Option<&Option<Blackboard::Instant>>,
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
