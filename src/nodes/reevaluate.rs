use core::marker::PhantomData;

use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, EntryMode, NodeResult, ReadFn};

/// A child resumed as Evaluate while a condition holds.
pub struct ReevaluateWhen<Condition, Child, Reads> {
    condition: Condition,
    child: Child,
    reads: PhantomData<fn() -> Reads>,
}

/// Passes `Resume` down to `child` as `Evaluate` on updates where
/// `condition` holds, so that subtree reconsiders its choices even when the
/// rest of the tree only resumes; otherwise passes the mode through.
///
/// For something that should make the agent reconsider, such as an alarm
/// changing: `reevaluate_when(|bb: &Guard| bb.alarm_changed, combat)`. The
/// condition is `Fn(&Context) -> bool` or `Fn(&Context, Params) -> bool`, see [`ReadFn`].
/// Never converts unconditionally: a subtree evaluated on every update is what
/// `Evaluate` at the root already gives.
pub fn reevaluate_when<Condition, Child, Reads>(
    condition: Condition,
    child: Child,
) -> ReevaluateWhen<Condition, Child, Reads> {
    ReevaluateWhen {
        condition,
        child,
        reads: PhantomData,
    }
}

impl<Context, Act, Params: ParamValue, Condition, Child, ChildState, ChildMemory, Reads>
    BtNode<Context, Act, Params> for ReevaluateWhen<Condition, Child, Reads>
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
    type State = ChildState;
    type Memory = ChildMemory;
    const NODES: usize =
        1 + <Child as BtNode<Context, Act, <Params::Shape as ParamShape>::Value<'static>>>::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut ChildState,
        memory: &mut ChildMemory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let mut params = params.into_value();
        let entry = if entry.mode() == EntryMode::Resume {
            let holds = self
                .condition
                .call(ctx, Params::Shape::reborrow(&mut params));
            entry.record("if", || holds);
            if holds {
                entry.with_mode(EntryMode::Evaluate)
            } else {
                entry
            }
        } else {
            entry
        };
        entry.run(1, &self.child, state, memory, ctx, params)
    }

    fn inspect(
        &self,
        state: Option<&ChildState>,
        memory: &ChildMemory,
        inspector: &mut dyn Inspector,
    ) {
        let node = NodeInfo::new("reevaluate_when", state.is_some()).with_fn_name::<Condition>();
        inspector.node(node, |inspector| {
            BtNode::<Context, Act, <Params::Shape as ParamShape>::Value<'_>>::inspect(
                &self.child,
                state,
                memory,
                inspector,
            );
        });
    }
}
