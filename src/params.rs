//! Parameter reborrowing for controls and actions.
//! Unit, references, and tuples are supported. Implement these traits for custom
//! parameter structs that must be borrowed across successive calls.

use core::marker::PhantomData;

/// Lifetime-indexed view, independent of its source scope.
/// Tuple shapes are generated through `FLATBT_MAX_PARAMS` (default 32).
pub trait ParamShape {
    type Value<'a>;

    /// Borrows the parameters for a shorter lifetime.
    fn reborrow<'a, 'b: 'a>(value: &'a mut Self::Value<'b>) -> Self::Value<'a>;
}

/// Maps a value to its reborrowable shape for successive child/callback calls.
/// Implemented for unit, references, and generated tuples. Leaves can accept
/// values without this trait.
pub trait ParamValue {
    type Shape: ParamShape;

    fn into_value<'a>(self) -> <Self::Shape as ParamShape>::Value<'a>
    where
        Self: 'a;
}

/// Shared input shape. Its view prevents direct mutation:
///
/// ```compile_fail,E0594
/// use flatbt::{BtNode, EntryMode, NodeResult};
/// struct InvalidWriter;
/// impl BtNode<(), (), &u32> for InvalidWriter {
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

/// Exclusive parameter shape; often an `Option<T>` output slot.
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
