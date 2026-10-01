use core::marker::PhantomData;

use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult, ReadFn};

/// A child restarted while a condition holds.
pub struct RepeatWhile<Condition, Child, Reads> {
    condition: Condition,
    child: Child,
    reads: PhantomData<fn() -> Reads>,
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
/// Parameters are forwarded to the child. The condition is `Fn(&Context) -> bool`,
/// or `Fn(&Context, Params) -> bool` to read them too, such as a target bound with
/// `.with(target)`; see [`ReadFn`].
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
///
/// Reading a target held in a `scope!` local:
///
/// ```
/// use flatbt::prelude::*;
///
/// struct World { at: u32 }
///
/// let tree = scope! {
///     let target: u32 = |_: &mut World| 3;
///     sequence {
///         repeat_while(
///             |world: &World, target: &u32| world.at < *target,
///             leaf_with(|_: &mut World, target: &u32| NodeResult::Running(*target)),
///         ).with(target);
///     }
/// };
/// let mut state = BtState::new(&tree);
/// let mut world = World { at: 0 };
/// assert_eq!(update(&tree, &mut state, &mut world, EntryMode::Resume), NodeResult::Running(3));
/// world.at = 3;
/// assert_eq!(update(&tree, &mut state, &mut world, EntryMode::Resume), NodeResult::Success);
/// ```
pub fn repeat_while<Condition, Child, Reads>(
    condition: Condition,
    child: Child,
) -> RepeatWhile<Condition, Child, Reads> {
    RepeatWhile {
        condition,
        child,
        reads: PhantomData,
    }
}

/// Child state, and whether it has returned `Running` since it started.
#[derive(Default)]
pub struct RepeatWhileState<ChildState> {
    child: ChildState,
    ran: bool,
}

impl<Context, Act, Params: ParamValue, Condition, Child, ChildState, ChildMemory, Reads>
    BtNode<Context, Act, Params> for RepeatWhile<Condition, Child, Reads>
where
    Condition: ReadFn<Context, Params, bool, Reads>,
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
    type State = RepeatWhileState<ChildState>;
    type Memory = ChildMemory;
    const NODES: usize =
        1 + <Child as BtNode<Context, Act, <Params::Shape as ParamShape>::Value<'static>>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut ChildMemory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let mut params = params.into_value();
        let holds = self
            .condition
            .call(ctx, Params::Shape::reborrow(&mut params));
        entry.record("while", || holds);
        if !holds {
            return NodeResult::Success;
        }
        let mut restarted = false;
        loop {
            let child_params = Params::Shape::reborrow(&mut params);
            // A restart is a fresh invocation of the child.
            let result = if restarted {
                entry.run_candidate(1, &self.child, &mut state.child, memory, ctx, child_params)
            } else {
                entry.run(1, &self.child, &mut state.child, memory, ctx, child_params)
            };
            if result.is_running() {
                state.ran = true;
                return result;
            }
            let ran = core::mem::take(&mut state.ran);
            state.child = ChildState::default();
            let holds = self
                .condition
                .call(ctx, Params::Shape::reborrow(&mut params));
            entry.record("while", || holds);
            if !holds {
                return NodeResult::Success;
            }
            match result {
                NodeResult::Success if ran => {}
                NodeResult::Success if restarted => {
                    return entry.error("repeat_while: restarted child completed without running");
                }
                _ => return NodeResult::Failure,
            }
            restarted = true;
        }
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &ChildMemory,
        inspector: &mut dyn Inspector,
    ) {
        let node = NodeInfo::new("repeat_while", state.is_some()).with_fn_name::<Condition>();
        inspector.node(node, |inspector| {
            BtNode::<Context, Act, <Params::Shape as ParamShape>::Value<'_>>::inspect(
                &self.child,
                state.map(|state| &state.child),
                memory,
                inspector,
            );
        });
    }
}
