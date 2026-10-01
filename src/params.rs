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
/// use flatbt::{BtNode, Entry, NodeResult};
/// struct InvalidWriter;
/// impl BtNode<(), (), &u32> for InvalidWriter {
///     type State = ();
///     type Memory = ();
///     fn update(&self, _: &mut (), _: &mut (), _: &mut (), input: &u32, _: Entry<'_>) -> NodeResult {
///         *input = 10;
///         NodeResult::Success
///     }
/// }
/// ```
pub struct Read<Target>(PhantomData<fn() -> Target>);
impl<Target: 'static> ParamShape for Read<Target> {
    type Value<'a> = &'a Target;

    fn reborrow<'a, 'b: 'a>(value: &'a mut &'b Target) -> &'a Target {
        value
    }
}

impl<Target: 'static> ParamValue for &Target {
    type Shape = Read<Target>;
    fn into_value<'a>(self) -> &'a Target
    where
        Self: 'a,
    {
        self
    }
}

/// Exclusive parameter shape; often an `Option<Target>` output slot.
pub struct Write<Target>(PhantomData<fn() -> Target>);
impl<Target: 'static> ParamShape for Write<Target> {
    type Value<'a> = &'a mut Target;

    fn reborrow<'a, 'b: 'a>(value: &'a mut &'b mut Target) -> &'a mut Target {
        value
    }
}

impl<Target: 'static> ParamValue for &mut Target {
    type Shape = Write<Target>;
    fn into_value<'a>(self) -> &'a mut Target
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
