//! Static child dispatch, control policies, and basic composition.

mod children;
mod control;
mod leaf;
mod read;

pub use children::{BtChildren, child_state};
pub use control::{
    BtControl, ControlNode, ControlOp, ControlState, Selector, Sequence, control, select, seq,
};
pub use leaf::{Check, Guarded, Leaf, check, guard, leaf};
pub use read::{ReadFn, ReadsContext, ReadsParams};
