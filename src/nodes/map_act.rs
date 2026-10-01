use core::marker::PhantomData;

use crate::inspect::{Inspector, NodeInfo};
use crate::{BtNode, Entry, NodeResult};

/// A child whose act is converted.
pub struct MapAct<Map, Child, ChildAct> {
    map: Map,
    child: Child,
    act: PhantomData<fn() -> ChildAct>,
}

/// Runs `child`, deciding `ChildAct`, in a tree deciding `Act`: its act passes through
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
pub fn map_act<Map, Child, ChildAct>(map: Map, child: Child) -> MapAct<Map, Child, ChildAct> {
    MapAct {
        map,
        child,
        act: PhantomData,
    }
}

impl<
    Context,
    Act,
    ChildAct,
    Params,
    Map: Fn(ChildAct) -> Act,
    Child: BtNode<Context, ChildAct, Params>,
> BtNode<Context, Act, Params> for MapAct<Map, Child, ChildAct>
{
    type State = Child::State;
    type Memory = Child::Memory;
    const NODES: usize = 1 + Child::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Child::State,
        memory: &mut Child::Memory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        match entry.run(1, &self.child, state, memory, ctx, params) {
            NodeResult::Running(act) => NodeResult::Running((self.map)(act)),
            NodeResult::Success => NodeResult::Success,
            NodeResult::Failure => NodeResult::Failure,
        }
    }

    fn inspect(
        &self,
        state: Option<&Child::State>,
        memory: &Child::Memory,
        inspector: &mut dyn Inspector,
    ) {
        let node = NodeInfo::new("map_act", state.is_some()).with_fn_name::<Map>();
        inspector.node(node, |inspector| {
            self.child.inspect(state, memory, inspector)
        });
    }
}
