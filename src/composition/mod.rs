//! Static child dispatch, control policies, and basic composition.
//!
//! Every builder follows [`crate::BtNode`]'s thinking and acting contract:
//! branches retain changes to the blackboard and node memory; the driver applies
//! the final act to the world.

mod children;
mod control;
mod leaf;
mod read;

pub use children::{BtChildren, child_state};
pub use control::{
    BtControl, ControlMemory, ControlNode, ControlOp, ControlState, Selector, Sequence, control,
    select, seq,
};
pub use leaf::{Check, Guarded, Leaf, check, guard, leaf};
pub use read::{ReadFn, ReadsBlackboard, ReadsParams};

#[doc(hidden)]
pub use read::ReadsContext;
