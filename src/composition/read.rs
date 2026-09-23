use crate::params::{ParamShape, ParamValue};

/// Marker: a callable that reads the context only.
pub struct ReadsContext;

/// Marker: a callable that reads the context and the node's parameters.
pub struct ReadsParams;

/// A callable over the context, optionally the node's parameters too.
///
/// Lets one constructor, such as [`guard`](crate::guard), take either shape:
///
/// - `Fn(&C) -> R`, marker [`ReadsContext`]: parameters are ignored.
/// - `Fn(&C, P) -> R`, marker [`ReadsParams`]: `P` is a reborrow of the
///   parameters the node receives, such as `&Target` from `.with(target)`.
///
/// The marker is inferred from the callable's arity; annotate a closure's
/// arguments so its shape is known. `P` must implement [`ParamValue`], as for
/// [`seq`](crate::seq) and [`select`](crate::select).
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot read this context and these parameters",
    note = "expected `Fn(&C) -> R`, or `Fn(&C, P) -> R` where `P` is what `.with(..)` binds; annotate the closure's arguments"
)]
pub trait ReadFn<C, P: ParamValue, R, M> {
    fn call(&self, ctx: &C, params: <P::Shape as ParamShape>::Value<'_>) -> R;
}

impl<C, P: ParamValue, R, F: Fn(&C) -> R> ReadFn<C, P, R, ReadsContext> for F {
    fn call(&self, ctx: &C, _: <P::Shape as ParamShape>::Value<'_>) -> R {
        self(ctx)
    }
}

impl<C, P: ParamValue, R, F> ReadFn<C, P, R, ReadsParams> for F
where
    F: for<'a> Fn(&C, <P::Shape as ParamShape>::Value<'a>) -> R,
{
    fn call(&self, ctx: &C, params: <P::Shape as ParamShape>::Value<'_>) -> R {
        self(ctx, params)
    }
}
