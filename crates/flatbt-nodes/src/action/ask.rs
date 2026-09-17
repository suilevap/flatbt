use crate::action::{ActionNode, BtAction, action};

/// A question put to whatever fills the context, waiting to be answered.
///
/// Built by [`ask`].
pub struct Ask<R, A> {
    request: R,
    answered: A,
}

/// Asks once, then runs until the answer is there.
///
/// A tree reads and writes its context and nothing else. When it needs
/// something the context does not hold yet -- a path, a line of sight, a nearest
/// anything -- the only thing it can do is *ask*: write the question where
/// whatever fills the context will see it, and wait. `ask` is that, as one node:
/// `request` runs once per invocation, and `is_in_progress` holds until
/// `answered` returns a value.
///
/// It is an action rather than a leaf because a leaf returning
/// [`Running`](flatbt_core::NodeResult::Running) is re-entered on every resume,
/// so a leaf that asks would ask again every update. `start` runs once, which is
/// what asking means. Nothing is asked when the answer already stands, so a
/// standing answer costs one call to `answered`.
///
/// Succeeds the moment the answer is there, so the node after it can use it.
///
/// ```
/// # use flatbt_core::{BtNode, BtState, EntryMode, NodeResult, seq, update};
/// # use flatbt_nodes::ask;
/// #[derive(Default)]
/// struct Agent {
///     wants_cover: bool,
///     cover: Option<u32>,
/// }
///
/// let tree = seq((ask(
///     |agent: &mut Agent| agent.wants_cover = true,
///     |agent: &Agent| agent.cover,
/// ),));
/// let mut state = BtState::new(&tree);
/// let mut agent = Agent::default();
///
/// // Nothing known yet: it asks, and waits.
/// assert_eq!(
///     update(&tree, &mut state, &mut agent, EntryMode::Evaluate),
///     NodeResult::Running
/// );
/// assert!(agent.wants_cover);
///
/// // Something else answers, and the next update completes.
/// agent.cover = Some(7);
/// assert_eq!(
///     update(&tree, &mut state, &mut agent, EntryMode::Resume),
///     NodeResult::Success
/// );
/// ```
///
/// # Into a `scope!` local
///
/// Bound to an output slot with `.with(out name)`, `ask` hands the answer to
/// the nodes after it as a plain value instead of leaving them to unwrap it:
///
/// ```ignore
/// scope! {
///     let spot: Vec2;
///     sequence {
///         ask(|a: &mut Agent| a.wants_cover = true, |a: &Agent| a.cover).with(out spot);
///         WalkTo.with(spot);
///     }
/// }
/// ```
///
/// `WalkTo` then takes a `Vec2` rather than an `Option<Vec2>`, so it cannot run
/// without one. The local belongs to the invocation, so leaving the branch and
/// coming back asks again instead of acting on an answer chosen for an older
/// situation. In that shape `answered` returns `Option<T>`; on its own it may
/// return `Option<T>` too, and the value is simply dropped.
pub fn ask<R, A>(request: R, answered: A) -> ActionNode<Ask<R, A>> {
    action(Ask { request, answered })
}

impl<C, R, A, T> BtAction<C> for Ask<R, A>
where
    R: Fn(&mut C),
    A: Fn(&C) -> Option<T>,
{
    type State = ();

    fn start(&self, ctx: &mut C, _: ()) -> Option<()> {
        if (self.answered)(ctx).is_none() {
            (self.request)(ctx);
        }
        Some(())
    }

    fn is_in_progress(&self, _: &(), ctx: &C, _: ()) -> bool {
        (self.answered)(ctx).is_none()
    }
}

impl<C, R, A, T> BtAction<C, &mut Option<T>> for Ask<R, A>
where
    R: Fn(&mut C),
    A: Fn(&C) -> Option<T>,
    T: 'static,
{
    type State = ();

    fn start(&self, ctx: &mut C, _: &mut Option<T>) -> Option<()> {
        if (self.answered)(ctx).is_none() {
            (self.request)(ctx);
        }
        Some(())
    }

    fn is_in_progress(&self, _: &(), ctx: &C, _: &mut Option<T>) -> bool {
        (self.answered)(ctx).is_none()
    }

    /// Fills the slot the moment the answer is there, so the nodes after this
    /// one read a value rather than an `Option`.
    fn complete(&self, _: &mut (), ctx: &mut C, slot: &mut Option<T>) -> bool {
        *slot = (self.answered)(ctx);
        true
    }
}
