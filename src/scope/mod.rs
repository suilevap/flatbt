//! Invocation-local values and parameter bindings.
//! Use [`scope!`] for named locals, or [`scope()`] and [`bind`] for function-based
//! construction. Scope owns data; its child controls execution.

mod binding;
mod macros;
mod storage;

#[doc(inline)]
pub use crate::__flatbt_scope as scope;
pub use crate::params::{Read, Write};
pub use binding::{
    Bound, ParamBinding, ParamsBinding, ReadBinding, WithParams, WithoutParams, WriteBinding, bind,
    no_params, params, read, write,
};
pub use storage::{Compute, Init, InitState, NoInit, Scope, ScopeInit, ScopeState, compute, scope};
