//! Parameter views shared by nodes, controls, and action adapters.
//! Ordinary nodes use unit or reference parameters directly. Implement these
//! traits only for custom parameter structs that composing nodes must reborrow.

use std::marker::PhantomData;

/// A lifetime-indexed parameter shape, independent of the scope that supplies it.
/// Implement this for application-defined structs containing several borrows.
/// Tuple shapes are generated through FLATBT_MAX_PARAMS (default 32).
pub trait ParamShape {
    type Value<'a>;

    /// Lends the same parameters to another callback without retaining its borrow.
    fn reborrow<'a, 'b: 'a>(value: &'a mut Self::Value<'b>) -> Self::Value<'a>;
}

/// Identifies how a parameter value can be lent repeatedly by a composing node.
/// Implemented for unit, shared/exclusive references, and generated tuples.
/// Leaf nodes accept ordinary values; this trait is needed by controls/actions
/// that invoke several children or callbacks using the same parameters.
pub trait ParamValue {
    type Shape: ParamShape;

    fn into_value<'a>(self) -> <Self::Shape as ParamShape>::Value<'a>
    where
        Self: 'a;
}

/// A shared input parameter.
/// The node cannot mutate the referenced input:
///
/// ```compile_fail,E0594
/// use flatbt_core::{BtNode, EntryMode, NodeResult};
/// struct InvalidWriter;
/// impl BtNode<(), &u32> for InvalidWriter {
///     type State = ();
///     fn update(&self, _: &mut (), _: &mut (), input: &u32, _: EntryMode) -> NodeResult {
///         *input = 10;
///         NodeResult::Success
///     }
/// }
/// ```
pub struct Read<T>(PhantomData<fn() -> T>);
impl<T: 'static> ParamShape for Read<T> {
    type Value<'a> = &'a T;

    fn reborrow<'a, 'b: 'a>(value: &'a mut &'b T) -> &'a T {
        value
    }
}

impl<T: 'static> ParamValue for &T {
    type Shape = Read<T>;
    fn into_value<'a>(self) -> &'a T
    where
        Self: 'a,
    {
        self
    }
}

/// An exclusive parameter, typically an output slot such as `Option<T>`.
pub struct Write<T>(PhantomData<fn() -> T>);
impl<T: 'static> ParamShape for Write<T> {
    type Value<'a> = &'a mut T;

    fn reborrow<'a, 'b: 'a>(value: &'a mut &'b mut T) -> &'a mut T {
        value
    }
}

impl<T: 'static> ParamValue for &mut T {
    type Shape = Write<T>;
    fn into_value<'a>(self) -> &'a mut T
    where
        Self: 'a,
    {
        self
    }
}

impl ParamShape for () {
    type Value<'a> = ();
    fn reborrow<'a, 'b: 'a>(_: &'a mut ()) {}
}

impl ParamValue for () {
    type Shape = ();
    fn into_value<'a>(self)
    where
        Self: 'a,
    {
    }
}

macro_rules! tuple_param_shapes {
    (@prefix [$($done:ident,)*]; $next:ident $(, $rest:ident)*) => {
        impl<$($done: ParamShape,)* $next: ParamShape> ParamShape for ($($done,)* $next,) {
            type Value<'a> = ($($done::Value<'a>,)* $next::Value<'a>,);
            #[allow(non_snake_case)]
            fn reborrow<'a, 'b: 'a>(value: &'a mut Self::Value<'b>) -> Self::Value<'a> {
                let ($($done,)* $next,) = value;
                ($($done::reborrow($done),)* $next::reborrow($next),)
            }
        }
        impl<$($done: ParamValue,)* $next: ParamValue> ParamValue for ($($done,)* $next,) {
            type Shape = ($($done::Shape,)* $next::Shape,);
            #[allow(non_snake_case)]
            fn into_value<'a>(self) -> <Self::Shape as ParamShape>::Value<'a> where Self: 'a {
                let ($($done,)* $next,) = self;
                ($($done.into_value(),)* $next.into_value(),)
            }
        }
        tuple_param_shapes!(@prefix [$($done,)* $next,]; $($rest),*);
    };
    (@prefix [$($done:ident,)*];) => {};
}

include!(concat!(env!("OUT_DIR"), "/tuple_params.rs"));
