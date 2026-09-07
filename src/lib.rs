//! A synchronous behavior tree runtime with static composition.
//!
//! This is the M0 reference implementation. Every call starts a fresh invocation;
//! suspension, persistent state, and post-commit effects are not implemented yet.
//!
//! ```
//! use flatbt::{BtNode, NodeResult, check, leaf, select, seq};
//!
//! struct Context { ready: bool, shots: usize }
//! let tree = select((
//!     seq((
//!         check(|ctx: &Context| ctx.ready),
//!         leaf(|ctx: &mut Context| {
//!             ctx.shots += 1;
//!             NodeResult::Success
//!         }),
//!     )),
//!     leaf(|_: &mut Context| NodeResult::Success),
//! ));
//! let mut ctx = Context { ready: true, shots: 0 };
//! assert_eq!(tree.update(&mut ctx), NodeResult::Success);
//! assert_eq!(ctx.shots, 1);
//! ```

#![forbid(unsafe_code)]

mod children;
mod control;
mod leaf;

pub use children::BtChildren;
pub use control::{BtControl, ControlNode, ControlOp, Selector, Sequence, control, select, seq};
pub use leaf::{Check, Leaf, check, leaf};

/// The terminal outcome of a synchronous invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum NodeResult {
    Success,
    Failure,
}

/// An immutable behavior definition operating on caller-owned context.
///
/// The M0 protocol is deliberately synchronous. Its signature will evolve when
/// persistent invocation state and execution traversal are introduced.
pub trait BtNode<C> {
    fn update(&self, ctx: &mut C) -> NodeResult;
}
