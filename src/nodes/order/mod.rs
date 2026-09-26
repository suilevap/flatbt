//! Children visited in a computed order.
//!
//! [`order_by`] wraps a tuple of children so that `select` and `seq` visit them
//! in the order a [`BtOrder`] decides, rather than as written. The control keeps
//! its own meaning: `select(order_by(..))` tries children in that order until
//! one succeeds, `seq(order_by(..))` runs them all in that order.
//!
//! ```
//! use flatbt::prelude::*;
//!
//! struct Needs { hunger: f32, fatigue: f32 }
//!
//! let tree = select(order_by(
//!     by_score(|needs: &Needs, index: usize| [needs.hunger, needs.fatigue][index]),
//!     (
//!         leaf(|_: &mut Needs| NodeResult::Running("eat")),
//!         leaf(|_: &mut Needs| NodeResult::Running("sleep")),
//!     ),
//! ));
//! let mut state = BtState::new(&tree);
//! let mut needs = Needs { hunger: 0.2, fatigue: 0.9 };
//! assert_eq!(update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(), Some("sleep"));
//! ```

mod per_child;
mod random;
mod score;

pub use random::{Shuffled, Weighted, shuffled, weighted};
pub use score::{ByScore, by_score};

use crate::{BtChildren, ControlNode, EntryMode, NodeResult, Selector, Sequence, select, seq};

/// Children used in an invocation are one bit each in a `u64`.
const MAX_CHILDREN: usize = 64;

/// Decides which child comes next.
///
/// `next` receives the children already used at earlier positions in this
/// pass, and the child still running from an earlier update, if any. It
/// returns the next child, or `None` when no child is left to offer; the
/// control then sees a Failure at that position.
///
/// `State` lives as long as the invocation and survives `Evaluate`.
pub trait BtOrder<C> {
    type State: Default + Send + 'static;

    /// Called with an empty `used` at the first position of each pass: on
    /// entry, and whenever `Evaluate` brings the control back to its start.
    fn next(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        used: u64,
        running: Option<usize>,
        child_count: usize,
    ) -> Option<usize>;
}

/// Children visited in the order a [`BtOrder`] decides.
pub struct Ordered<O, Children> {
    order: O,
    children: Children,
}

/// Wraps `children` so that a control visits them in the order `order`
/// decides. See the [module](self) for the orders and how controls use them.
///
/// Position `k` of the control is the `k`-th child the order offers. The
/// order is asked again from the start whenever the control goes back to
/// position 0 under `Evaluate`: `select` does on every `Evaluate`, so a score
/// order can preempt the running child; `seq` does only while still on its
/// first child. `Resume` never reorders.
///
/// Only controls that visit positions in order -- 0, the running one, or the
/// next -- are supported, as `select` and `seq` do. Any other position reports
/// a diagnostic and fails. At most 64 children; more fails to build.
pub fn order_by<O, Children>(order: O, children: Children) -> Ordered<O, Children> {
    Ordered { order, children }
}

/// Inner children, order state, and the position being visited.
#[derive(Default)]
pub struct OrderedState<OrderState, ChildrenState> {
    // Drop descendants first, as the rest of the tree does.
    children: ChildrenState,
    order: OrderState,
    /// Children used at earlier positions of this pass.
    used: u64,
    /// The position being visited and the child the order put there, if any.
    at: Option<(u8, Option<u8>)>,
}

/// Kept cold and out of line, so the formatting stays off the path every
/// update takes.
#[cold]
#[inline(never)]
fn unsupported<A>(position: usize, at: Option<usize>) -> NodeResult<A> {
    NodeResult::error(format_args!(
        "order_by visits positions in order; asked for {position} after {at:?}"
    ))
}

impl<C, A, P, O, Children> BtChildren<C, A, P> for Ordered<O, Children>
where
    O: BtOrder<C>,
    Children: BtChildren<C, A, P>,
{
    type State = OrderedState<O::State, Children::State>;
    const LEN: usize = Children::LEN;

    #[inline]
    fn active_child_index(&self, state: &Self::State) -> Option<usize> {
        self.children
            .active_child_index(&state.children)
            .and(state.at.map(|(position, _)| position as usize))
    }

    #[inline]
    fn run_child(
        &self,
        state: &mut Self::State,
        position: usize,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult<A> {
        // The child count is static, so too many is a build error, not a check
        // on every update.
        const {
            assert!(
                Children::LEN <= MAX_CHILDREN,
                "order_by supports at most 64 children"
            )
        };
        let at = state.at.map(|(at, _)| at as usize);
        let child = match state.at {
            // A new pass: on entry, or Evaluate back at the start.
            None if position == 0 => self.pick(state, ctx, 0),
            _ if position == 0 && mode == EntryMode::Evaluate => self.pick(state, ctx, 0),
            // The same position again: Resume, or Evaluate continuing it.
            Some((at, child)) if at as usize == position => child,
            // The next position.
            Some((at, child)) if at as usize + 1 == position => {
                if let Some(child) = child {
                    state.used |= 1 << child;
                }
                self.pick(state, ctx, position)
            }
            _ => return unsupported(position, at),
        };
        match child {
            Some(child) => {
                self.children
                    .run_child(&mut state.children, child as usize, ctx, params, mode)
            }
            None => NodeResult::Failure,
        }
    }
}

impl<O, Children> Ordered<O, Children> {
    /// Asks the order for the child at `position` and records it.
    #[inline]
    fn pick<C, A, P>(
        &self,
        state: &mut OrderedState<O::State, Children::State>,
        ctx: &mut C,
        position: usize,
    ) -> Option<u8>
    where
        O: BtOrder<C>,
        Children: BtChildren<C, A, P>,
    {
        if position == 0 {
            state.used = 0;
        }
        let running = self.children.active_child_index(&state.children);
        let child = self
            .order
            .next(&mut state.order, ctx, state.used, running, Children::LEN)
            .filter(|child| *child < Children::LEN && state.used & (1 << child) == 0)
            .map(|child| child as u8);
        state.at = Some((position as u8, child));
        child
    }
}

// Shorthands for the common pairings. Each is exactly the composition it names.

/// `select(order_by(by_score(score), children))`: a utility selector. For
/// inertia, write the composition with `by_score(score).inertia(x)`;
/// [`crate::per_child!`] writes the scores next to their children.
pub fn utility<F, S, Children>(
    score: F,
    children: Children,
) -> ControlNode<Selector, Ordered<ByScore<F, S>, Children>> {
    select(order_by(by_score(score), children))
}

/// `select(order_by(shuffled(rng), children))`: runs a random child, falling
/// back to a random one of the rest.
pub fn random_select<R, Children>(
    rng: R,
    children: Children,
) -> ControlNode<Selector, Ordered<Shuffled<R>, Children>> {
    select(order_by(shuffled(rng), children))
}

/// `select(order_by(weighted(rng, weight), children))`: runs a child drawn by
/// weight, falling back to one drawn from the rest.
pub fn weighted_select<R, W, Children>(
    rng: R,
    weight: W,
    children: Children,
) -> ControlNode<Selector, Ordered<Weighted<R, W>, Children>> {
    select(order_by(weighted(rng, weight), children))
}

/// `seq(order_by(shuffled(rng), children))`: runs every child in a random
/// order.
pub fn shuffle_seq<R, Children>(
    rng: R,
    children: Children,
) -> ControlNode<Sequence, Ordered<Shuffled<R>, Children>> {
    seq(order_by(shuffled(rng), children))
}
