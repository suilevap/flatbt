//! Invocation-local values and parameter bindings.
//! Use [`scope!`] for named locals, or [`scope()`] and [`bind`] for function-based
//! construction. Scope owns data; its child controls execution.

#![forbid(unsafe_code)]

mod binding;
mod macros;
mod storage;

#[doc(inline)]
pub use crate::__flatbt_scope as scope;
pub use binding::{
    Bound, ParamBinding, ParamsBinding, ReadBinding, WithParams, WithoutParams, WriteBinding, bind,
    no_params, params, read, write,
};
pub use flatbt_core::params::{Read, Write};
pub use storage::{Compute, Scope, ScopeState, compute, scope};

#[doc(hidden)]
pub mod __private {
    pub use flatbt_core as core;
}
