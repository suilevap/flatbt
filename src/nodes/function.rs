use core::marker::PhantomData;

use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult, ReadFn};

/// Stateless callable that also receives parameters.
pub struct LeafWith<Function>(Function);

/// [`leaf`](crate::leaf) whose callable also receives the node's parameters,
/// so inside `scope!` it reads and writes locals through `.with(..)`.
///
/// Annotate the arguments; borrowed parameters then stay open to every
/// lifetime a binding hands out.
///
/// ```
/// use flatbt::prelude::*;
///
/// let tree = scope! {
///     let target: u32 = |_: &mut Vec<u32>| 7;
///     sequence {
///         leaf_with(|log: &mut Vec<u32>, target: &u32| {
///             log.push(*target);
///             NodeResult::Success
///         }).with(target);
///     }
/// };
/// let mut state: BtState<_, _> = BtState::new(&tree);
/// let mut log = Vec::new();
/// assert_eq!(update(&tree, &mut state, &mut log, EntryMode::Evaluate), NodeResult::Success);
/// assert_eq!(log, [7]);
/// ```
pub fn leaf_with<Function>(f: Function) -> LeafWith<Function> {
    LeafWith(f)
}

impl<Context, Act, Params, Function: Fn(&mut Context, Params) -> NodeResult<Act>>
    BtNode<Context, Act, Params> for LeafWith<Function>
{
    type State = ();
    type Memory = ();

    #[inline]
    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        params: Params,
        _: Entry<'_>,
    ) -> NodeResult<Act> {
        (self.0)(ctx, params)
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("leaf_with", state.is_some()).with_fn_name::<Function>();
        inspector.node(node, |_| {});
    }
}

/// Predicate over shared context and parameters.
pub struct CheckWith<Predicate>(Predicate);

/// [`check`](crate::check) whose predicate also receives the node's
/// parameters. Asked once, on entry, like `check`.
pub fn check_with<Predicate>(predicate: Predicate) -> CheckWith<Predicate> {
    CheckWith(predicate)
}

impl<Context, Act, Params, Predicate: Fn(&Context, Params) -> bool> BtNode<Context, Act, Params>
    for CheckWith<Predicate>
{
    type State = ();
    type Memory = ();

    #[inline]
    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        params: Params,
        _: Entry<'_>,
    ) -> NodeResult<Act> {
        if (self.0)(ctx, params) {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("check_with", state.is_some()).with_fn_name::<Predicate>();
        inspector.node(node, |_| {});
    }
}

/// An act reported while a condition holds.
pub struct ActionWhile<Condition, MakeAct, Reads> {
    condition: Condition,
    act: MakeAct,
    reads: PhantomData<fn() -> Reads>,
}

/// Reports `act` while `condition` holds, then succeeds.
///
/// The most common action, without a [`BtAction`](crate::BtAction) impl: the
/// world does the work, and this says what the agent is doing until it is
/// done. Waiting for something is the same node with the condition negated.
/// Both are asked on every update, so `act` follows a moving target.
///
/// ```
/// use flatbt::prelude::*;
///
/// #[derive(Debug, PartialEq)]
/// enum Act { Reload }
///
/// let tree = action_while(|ammo: &u32| *ammo < 3, |_: &u32| Act::Reload);
/// let mut state = BtState::new(&tree);
/// let mut ammo = 2;
/// assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Running(Act::Reload));
/// ammo = 3;
/// assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Success);
/// ```
///
/// Each closure is `Fn(&Context)`, or `Fn(&Context, Params)` to also read the node's
/// parameters, such as a target held in a `scope!` local; see [`ReadFn`]:
///
/// ```
/// use flatbt::prelude::*;
///
/// #[derive(Debug, PartialEq)]
/// enum Act { WalkTo(u32) }
/// struct World { at: u32, positions: [u32; 2] }
///
/// let tree = scope! {
///     let target: usize = |_: &mut World| 1;
///     sequence {
///         action_while(
///             |world: &World, target: &usize| world.at != world.positions[*target],
///             |world: &World, target: &usize| Act::WalkTo(world.positions[*target]),
///         ).with(target);
///     }
/// };
/// let mut state = BtState::new(&tree);
/// let mut world = World { at: 0, positions: [2, 5] };
/// assert_eq!(update(&tree, &mut state, &mut world, EntryMode::Resume), NodeResult::Running(Act::WalkTo(5)));
/// world.at = 5;
/// assert_eq!(update(&tree, &mut state, &mut world, EntryMode::Resume), NodeResult::Success);
/// ```
pub fn action_while<Condition, MakeAct, Reads>(
    condition: Condition,
    act: MakeAct,
) -> ActionWhile<Condition, MakeAct, Reads> {
    ActionWhile {
        condition,
        act,
        reads: PhantomData,
    }
}

impl<Context, Act, Params: ParamValue, Condition, MakeAct, ConditionReads, ActReads>
    BtNode<Context, Act, Params> for ActionWhile<Condition, MakeAct, (ConditionReads, ActReads)>
where
    Condition: ReadFn<Context, Params, bool, ConditionReads>,
    MakeAct: ReadFn<Context, Params, Act, ActReads>,
{
    type State = ();
    type Memory = ();

    #[inline]
    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let mut params = params.into_value();
        let holds = self
            .condition
            .call(ctx, Params::Shape::reborrow(&mut params));
        entry.record("while", || holds);
        if holds {
            NodeResult::Running(self.act.call(ctx, params))
        } else {
            NodeResult::Success
        }
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        let node = NodeInfo::new("action_while", state.is_some()).with_fn_name::<Condition>();
        inspector.node(node, |_| {});
    }
}
