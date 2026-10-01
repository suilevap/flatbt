use core::marker::PhantomData;

use super::read::ReadFn;
use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult};

/// Stateless callable. Captures hold configuration; context holds mutable data.
pub struct Leaf<Function>(Function);

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
pub fn leaf<Function>(f: Function) -> Leaf<Function> {
    Leaf(f)
}

impl<Context, Act, Params, Function: Fn(&mut Context) -> NodeResult<Act>>
    BtNode<Context, Act, Params> for Leaf<Function>
{
    type State = ();
    type Memory = ();

    #[inline(always)]
    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        _: Params,
        _: Entry<'_>,
    ) -> NodeResult<Act> {
        (self.0)(ctx)
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("leaf", state.is_some()).with_fn_name::<Function>();
        inspector.node(node, |_| {});
    }
}

/// Predicate over shared context.
pub struct Check<Predicate>(Predicate);

/// Returns Success for true, Failure for false. Checked where the tree runs,
/// like [`leaf`].
///
/// A predicate never occupies the agent, so it never names the act type.
pub fn check<Predicate>(predicate: Predicate) -> Check<Predicate> {
    Check(predicate)
}

impl<Context, Act, Params, Predicate: Fn(&Context) -> bool> BtNode<Context, Act, Params>
    for Check<Predicate>
{
    type State = ();
    type Memory = ();

    #[inline(always)]
    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        _: Params,
        _: Entry<'_>,
    ) -> NodeResult<Act> {
        if (self.0)(ctx) {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("check", state.is_some()).with_fn_name::<Predicate>();
        inspector.node(node, |_| {});
    }
}

/// A child that runs only while a predicate holds.
pub struct Guarded<Predicate, Child, Reads> {
    predicate: Predicate,
    child: Child,
    reads: PhantomData<fn() -> Reads>,
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
/// The predicate is `Fn(&Context) -> bool`, or `Fn(&Context, Params) -> bool` to also read the
/// parameters it forwards to `child` -- a target bound with `.with(target)`,
/// asked about on every update. See [`ReadFn`].
///
/// [`seq`]: crate::seq
pub fn guard<Predicate, Child, Reads>(
    predicate: Predicate,
    child: Child,
) -> Guarded<Predicate, Child, Reads> {
    Guarded {
        predicate,
        child,
        reads: PhantomData,
    }
}

impl<Context, Act, Params: ParamValue, Predicate, Child, ChildState, ChildMemory, Reads>
    BtNode<Context, Act, Params> for Guarded<Predicate, Child, Reads>
where
    Predicate: ReadFn<Context, Params, bool, Reads>,
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

    #[inline(always)]
    fn update(
        &self,
        state: &mut ChildState,
        memory: &mut ChildMemory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let mut params = params.into_value();
        let holds = self
            .predicate
            .call(ctx, Params::Shape::reborrow(&mut params));
        entry.record("if", || holds);
        if holds {
            entry.run(1, &self.child, state, memory, ctx, params)
        } else {
            NodeResult::Failure
        }
    }

    fn inspect(
        &self,
        state: Option<&ChildState>,
        memory: &ChildMemory,
        inspector: &mut dyn Inspector,
    ) {
        let node = NodeInfo::new("guard", state.is_some()).with_fn_name::<Predicate>();
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
