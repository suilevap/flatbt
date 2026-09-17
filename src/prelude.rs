//! Tree authoring APIs. Optional exports follow enabled Cargo features.

pub use flatbt_core::params::{ParamShape, ParamValue, Read, Write};
pub use flatbt_core::{
    BtChildren, BtControl, BtNode, BtState, Check, ControlNode, ControlOp, ControlState, EntryMode,
    Leaf, NodeResult, Selector, Sequence, check, control, leaf, select, seq, update,
};

#[cfg(feature = "action")]
pub use flatbt_nodes::{ActionNode, Ask, BtAction, BtCancel, CancelOnDrop, Request, action, ask};
#[cfg(feature = "choose")]
pub use flatbt_nodes::{Choose, ChooseNode, choose};
#[cfg(feature = "scope")]
pub use flatbt_scope::{
    Bound, Compute, ParamBinding, ParamsBinding, ReadBinding, Scope, ScopeState, WithParams,
    WithoutParams, WriteBinding, bind, compute, no_params, params, read, scope, write,
};
