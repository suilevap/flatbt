use crate::{BtNode, EntryMode, NodeResult};

/// Stateless callable that also receives parameters.
pub struct LeafWith<F>(F);

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
pub fn leaf_with<F>(f: F) -> LeafWith<F> {
    LeafWith(f)
}

impl<C, A, P, F: Fn(&mut C, P) -> NodeResult<A>> BtNode<C, A, P> for LeafWith<F> {
    type State = ();

    fn update(&self, _: &mut (), ctx: &mut C, params: P, _: EntryMode) -> NodeResult<A> {
        (self.0)(ctx, params)
    }
}

/// Predicate over shared context and parameters.
pub struct CheckWith<F>(F);

/// [`check`](crate::check) whose predicate also receives the node's
/// parameters. Asked once, on entry, like `check`.
pub fn check_with<F>(predicate: F) -> CheckWith<F> {
    CheckWith(predicate)
}

impl<C, A, P, F: Fn(&C, P) -> bool> BtNode<C, A, P> for CheckWith<F> {
    type State = ();

    fn update(&self, _: &mut (), ctx: &mut C, params: P, _: EntryMode) -> NodeResult<A> {
        if (self.0)(ctx, params) {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    }
}

/// An act reported while a condition holds.
pub struct ActionWhile<F, G> {
    condition: F,
    act: G,
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
pub fn action_while<F, G>(condition: F, act: G) -> ActionWhile<F, G> {
    ActionWhile { condition, act }
}

impl<C, A, P, F: Fn(&C) -> bool, G: Fn(&C) -> A> BtNode<C, A, P> for ActionWhile<F, G> {
    type State = ();

    fn update(&self, _: &mut (), ctx: &mut C, _: P, _: EntryMode) -> NodeResult<A> {
        if (self.condition)(ctx) {
            NodeResult::Running((self.act)(ctx))
        } else {
            NodeResult::Success
        }
    }
}
