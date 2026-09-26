use core::marker::PhantomData;

use crate::inspect::{Inspector, NodeInfo, fn_name};
use crate::{BtNode, EntryMode, NodeResult};

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

    #[inline]
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

    fn inspect(&self, state: Option<&N::State>, inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("map_act", state.is_some()).name(fn_name::<F>());
        inspector.node(node, |inspector| self.child.inspect(state, inspector));
    }
}
