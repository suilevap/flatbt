use crate::params::{ParamShape, ParamValue};

/// Marker: a callable that reads the blackboard only.
pub struct ReadsBlackboard;

/// Compatibility name for [`ReadsBlackboard`].
#[doc(hidden)]
pub use ReadsBlackboard as ReadsContext;

/// Marker: a callable that reads the blackboard and the node's parameters.
pub struct ReadsParams;

/// A callable over the blackboard, optionally the node's parameters too.
///
/// Lets one constructor, such as [`guard`](crate::guard), take either shape:
///
/// - `Fn(&Blackboard) -> Output`, marker [`ReadsBlackboard`]: parameters are ignored.
/// - `Fn(&Blackboard, Params) -> Output`, marker [`ReadsParams`]: `Params` is a reborrow of the
///   parameters the node receives, such as `&Target` from `.with(target)`.
///
/// The marker is inferred from the callable's arity; annotate a closure's
/// arguments so its shape is known. `Params` must implement [`ParamValue`], as for
/// [`seq`](crate::seq) and [`select`](crate::select).
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot read this blackboard and these parameters",
    note = "expected `Fn(&Blackboard) -> Output`, or `Fn(&Blackboard, Params) -> Output` where `Params` is what `.with(..)` binds; annotate the closure's arguments"
)]
pub trait ReadFn<Blackboard, Params: ParamValue, Output, Reads> {
    fn call(&self, bb: &Blackboard, params: <Params::Shape as ParamShape>::Value<'_>) -> Output;
}

impl<Blackboard, Params: ParamValue, Output, Function: Fn(&Blackboard) -> Output>
    ReadFn<Blackboard, Params, Output, ReadsBlackboard> for Function
{
    #[inline(always)]
    fn call(&self, bb: &Blackboard, _: <Params::Shape as ParamShape>::Value<'_>) -> Output {
        self(bb)
    }
}

impl<Blackboard, Params: ParamValue, Output, Function>
    ReadFn<Blackboard, Params, Output, ReadsParams> for Function
where
    Function: for<'a> Fn(&Blackboard, <Params::Shape as ParamShape>::Value<'a>) -> Output,
{
    #[inline(always)]
    fn call(&self, bb: &Blackboard, params: <Params::Shape as ParamShape>::Value<'_>) -> Output {
        self(bb, params)
    }
}
