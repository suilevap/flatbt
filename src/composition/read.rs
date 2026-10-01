use crate::params::{ParamShape, ParamValue};

/// Marker: a callable that reads the context only.
pub struct ReadsContext;

/// Marker: a callable that reads the context and the node's parameters.
pub struct ReadsParams;

/// A callable over the context, optionally the node's parameters too.
///
/// Lets one constructor, such as [`guard`](crate::guard), take either shape:
///
/// - `Fn(&Context) -> Output`, marker [`ReadsContext`]: parameters are ignored.
/// - `Fn(&Context, Params) -> Output`, marker [`ReadsParams`]: `Params` is a reborrow of the
///   parameters the node receives, such as `&Target` from `.with(target)`.
///
/// The marker is inferred from the callable's arity; annotate a closure's
/// arguments so its shape is known. `Params` must implement [`ParamValue`], as for
/// [`seq`](crate::seq) and [`select`](crate::select).
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot read this context and these parameters",
    note = "expected `Fn(&Context) -> Output`, or `Fn(&Context, Params) -> Output` where `Params` is what `.with(..)` binds; annotate the closure's arguments"
)]
pub trait ReadFn<Context, Params: ParamValue, Output, Reads> {
    fn call(&self, ctx: &Context, params: <Params::Shape as ParamShape>::Value<'_>) -> Output;
}

impl<Context, Params: ParamValue, Output, Function: Fn(&Context) -> Output>
    ReadFn<Context, Params, Output, ReadsContext> for Function
{
    #[inline(always)]
    fn call(&self, ctx: &Context, _: <Params::Shape as ParamShape>::Value<'_>) -> Output {
        self(ctx)
    }
}

impl<Context, Params: ParamValue, Output, Function> ReadFn<Context, Params, Output, ReadsParams>
    for Function
where
    Function: for<'a> Fn(&Context, <Params::Shape as ParamShape>::Value<'a>) -> Output,
{
    #[inline(always)]
    fn call(&self, ctx: &Context, params: <Params::Shape as ParamShape>::Value<'_>) -> Output {
        self(ctx, params)
    }
}
