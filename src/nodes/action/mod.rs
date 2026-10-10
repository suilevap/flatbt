//! Action lifecycle adapter, cancellation owned by invocation state, and
//! waiting on futures.

mod cancel;
mod future;
mod lifecycle;

pub use cancel::{BtCancel, CancelOnDrop};
pub use future::{AwaitFuture, Awaiting, await_future};
pub use lifecycle::{ActionNode, BtAction, action};
