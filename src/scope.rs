//! Invocation-local values and explicit bindings to node parameters.
//!
//! Import [`scope!`] for named locals, or use [`scope()`] and [`bind`] to build
//! the same tree with ordinary functions. Scope owns data; its child controls
//! execution. Ordinary trees need no imports from this module.

mod binding;

pub use crate::params::{Read, Write};
pub use binding::{
    Bound, ParamBinding, ParamsBinding, ReadBinding, WithParams, WithoutParams, WriteBinding, bind,
    no_params, params, read, write,
};

use std::marker::PhantomData;

use crate::{BtNode, EntryMode, NodeResult};

/// Owns invocation-local data separately from the application context.
pub struct Scope<L, N> {
    child: N,
    locals: PhantomData<fn() -> L>,
}

/// Creates a scope whose local fields are initialized with Default on entry.
/// Producers can fill output slots before consumers read them through `bind`.
/// Each binding explicitly selects fields; equal field types do not imply sharing.
/// The enclosing parameter value is not implicitly inherited by a nested scope.
pub fn scope<L, N>(child: N) -> Scope<L, N> {
    Scope {
        child,
        locals: PhantomData,
    }
}

/// Descendants drop before the values they use. The complete layout is inline.
#[derive(Default)]
pub struct ScopeState<L, S> {
    child: S,
    locals: L,
}

impl<C, P, L, N, S> BtNode<C, P> for Scope<L, N>
where
    L: Default + Send + 'static,
    N: for<'a> BtNode<C, &'a mut L, State = S>,
    S: Default + Send + 'static,
{
    type State = ScopeState<L, S>;

    fn update(&self, state: &mut Self::State, ctx: &mut C, _: P, mode: EntryMode) -> NodeResult {
        BtNode::<C, &mut L>::update(&self.child, &mut state.child, ctx, &mut state.locals, mode)
    }
}

/// Adapts a synchronous initializer to an output node. The callback runs when
/// entered, not when the tree is constructed. scope! puts these nodes in an
/// initialization prefix that completes before entering the chosen control.
pub struct Compute<F>(F);

/// Computes one value from context and writes it to the supplied output slot.
pub fn compute<C, T, F: Fn(&mut C) -> T>(init: F) -> Compute<F> {
    Compute(init)
}

impl<C, T, F: Fn(&mut C) -> T> BtNode<C, &mut Option<T>> for Compute<F> {
    type State = ();

    fn update(&self, _: &mut (), ctx: &mut C, output: &mut Option<T>, _: EntryMode) -> NodeResult {
        *output = Some((self.0)(ctx));
        NodeResult::Success
    }
}

/// Owns named locals with synchronous initializers and explicit control flow.
///
/// `let name: Type = callback;` invokes Fn(&mut Context) -> Type once on entry.
/// `let name: Type;` reserves a slot for a suspending producer. The body must be
/// sequence { ... } or select { ... }; these controls can nest using the same
/// locals. `Node.with(name);` binds a shared input; `out name` lends an exclusive
/// slot. `node_expression.with(args);` supports configured nodes and actions.
/// Plain node expressions such as `wait_frames(1);` use unit parameters, with no
/// extra syntax. Constructors keep ordinary Rust arguments; `.with(...)` names
/// runtime local fields. Tuple bindings use FLATBT_MAX_PARAMS.
/// An optional `context: Type;` header lets initializer closures infer their
/// context argument type. Without it, annotate closure parameters explicitly.
///
/// Initializers run in declaration order before the body. They are not replayed
/// on Resume or Evaluate of a running scope. Node/closure definitions are built
/// once. Missing inputs fail at runtime; aliasing writes are rejected by Rust.
///
/// ```
/// use flatbt::{BtNode, BtState, EntryMode, NodeResult, update};
/// use flatbt::scope::scope;
/// struct Observe;
/// impl BtNode<Vec<u32>, &u32> for Observe {
///     type State = ();
///     fn update(&self, _: &mut (), ctx: &mut Vec<u32>, input: &u32, _: EntryMode) -> NodeResult {
///         ctx.push(*input);
///         NodeResult::Success
///     }
/// }
/// let tree = scope! {
///     context: Vec<u32>;
///     let walk_pos: u32 = |_| 10;
///     let door_pos: u32 = |_| 90;
///     sequence {
///         Observe.with(door_pos);
///         Observe.with(walk_pos);
///     }
/// };
/// let mut state = BtState::new(&tree);
/// let mut trace = Vec::new();
/// assert_eq!(update(&tree, &mut state, &mut trace, EntryMode::Resume), NodeResult::Success);
/// assert_eq!(trace, [90, 10]);
/// ```
///
/// Two exclusive parameters cannot borrow the same slot:
///
/// ```compile_fail,E0499
/// use flatbt::{BtNode, BtState, EntryMode, NodeResult};
/// use flatbt::scope::scope;
/// struct TwoOutputs;
/// impl BtNode<(), (&mut Option<u32>, &mut Option<u32>)> for TwoOutputs {
///     type State = ();
///     fn update(&self, _: &mut (), _: &mut (), _: (&mut Option<u32>, &mut Option<u32>), _: EntryMode) -> NodeResult {
///         NodeResult::Success
///     }
/// }
/// let tree = scope! {
///     let value: u32;
///     sequence { TwoOutputs.with(out value, out value); }
/// };
/// let _state = BtState::new(&tree);
/// ```
#[doc(inline)]
pub use crate::__flatbt_scope as scope;

#[doc(hidden)]
#[macro_export]
macro_rules! __flatbt_scope {
    (@locals [$locals:ident $value:ident $context:ty] [$($fields:tt)*] [$($init:tt)*];
        let $field:ident: $ty:ty = $callback:expr; $($rest:tt)*) => {
        $crate::scope::scope!(@locals [$locals $value $context]
            [$($fields)* $field: ::core::option::Option<$ty>,]
            [$($init)* $crate::scope::bind($crate::scope::compute::<$context, $ty, _>($callback),
                $crate::scope::write(|$value: &mut $locals| &mut $value.$field)),]; $($rest)*)
    };
    (@locals [$locals:ident $value:ident $context:ty] [$($fields:tt)*] [$($init:tt)*];
        let $field:ident: $ty:ty; $($rest:tt)*) => {
        $crate::scope::scope!(@locals [$locals $value $context]
            [$($fields)* $field: ::core::option::Option<$ty>,] [$($init)*]; $($rest)*)
    };
    (@locals [$locals:ident $value:ident $context:ty] [$($fields:tt)*] [$($init:tt)*];
        $control:ident { $($body:tt)* } $(;)?) => {{
        #[derive(Default)]
        struct $locals { $($fields)* }
        $crate::scope::scope::<$locals, _>($crate::scope::scope!(@initialized [$($init)*]
            $crate::scope::scope!(@children [$locals $value $context] $control []; $($body)*)))
    }};
    (@initialized [] $body:expr) => { $body };
    (@initialized [$($init:tt)+] $body:expr) => { $crate::seq(($($init)+ $body,)) };
    (@children $setup:tt $control:ident [$($nodes:tt)*]; sequence { $($body:tt)* } $($rest:tt)*) => {
        $crate::scope::scope!(@children $setup $control
            [$($nodes)* $crate::scope::scope!(@children $setup sequence []; $($body)*),]; $($rest)*)
    };
    (@children $setup:tt $control:ident [$($nodes:tt)*]; select { $($body:tt)* } $($rest:tt)*) => {
        $crate::scope::scope!(@children $setup $control
            [$($nodes)* $crate::scope::scope!(@children $setup select []; $($body)*),]; $($rest)*)
    };
    (@children $setup:tt $control:ident $nodes:tt; ; $($rest:tt)*) => {
        $crate::scope::scope!(@children $setup $control $nodes; $($rest)*)
    };
    (@children [$locals:ident $value:ident $context:ty] sequence [$($nodes:tt)*];) => { $crate::seq(($($nodes)*)) };
    (@children [$locals:ident $value:ident $context:ty] select [$($nodes:tt)*];) => { $crate::select(($($nodes)*)) };
    (@children $setup:tt $control:ident $nodes:tt; $($rest:tt)+) => {
        $crate::scope::scope!(@expression $setup $control $nodes []; $($rest)+)
    };
    // Recognize only a final .with(...) suffix. Everything before it remains an
    // ordinary Rust expression; nested calls, closures, and blocks stay opaque.
    (@expression $setup:tt $control:ident [$($nodes:tt)*] [$($node:tt)+];
        . with ($($args:tt)*) ; $($rest:tt)*) => {
        $crate::scope::scope!(@children $setup $control
            [$($nodes)* $crate::scope::scope!(@args $setup [$($node)+] []; $($args)*),]; $($rest)*)
    };
    (@expression $setup:tt $control:ident [$($nodes:tt)*] [$($node:tt)+]; ; $($rest:tt)*) => {
        $crate::scope::scope!(@children $setup $control
            [$($nodes)* $crate::scope::no_params($($node)+),]; $($rest)*)
    };
    (@expression $setup:tt $control:ident $nodes:tt [$($node:tt)*]; $next:tt $($rest:tt)*) => {
        $crate::scope::scope!(@expression $setup $control $nodes [$($node)* $next]; $($rest)*)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; out $field:ident $(, $($rest:tt)*)?) => {
        $crate::scope::scope!(@args $setup $node [$($args)* out $field,]; $($($rest)*)?)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; in $field:ident $(, $($rest:tt)*)?) => {
        $crate::scope::scope!(@args $setup $node [$($args)* in $field,]; $($($rest)*)?)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; $field:ident $(, $($rest:tt)*)?) => {
        $crate::scope::scope!(@args $setup $node [$($args)* in $field,]; $($($rest)*)?)
    };
    (@args $setup:tt [$node:expr] [];) => { $crate::scope::no_params($node) };
    (@args $setup:tt [$node:expr] [$($access:ident $field:ident,)+];) => {
        $crate::scope::scope!(@bind $setup $node; $($access $field),+)
    };
    (@bind [$locals:ident $value:ident $context:ty] $node:expr; in $field:ident) => {
        $crate::scope::bind($node, $crate::scope::read(|$value: &$locals| $value.$field.as_ref()))
    };
    (@bind [$locals:ident $value:ident $context:ty] $node:expr; out $field:ident) => {
        $crate::scope::bind($node, $crate::scope::write(|$value: &mut $locals| &mut $value.$field))
    };
    (@bind [$locals:ident $value:ident $context:ty] $node:expr; $($access:ident $field:ident),+) => {
        $crate::scope::bind($node, $crate::scope::params::<($($crate::scope::scope!(@shape $access),)+), _, _>(
            |$value: &mut $locals| ::core::option::Option::Some(($($crate::scope::scope!(@borrow $value $access $field),)+))
        ))
    };
    (@shape in) => { $crate::scope::Read<_> };
    (@shape out) => { $crate::scope::Write<_> };
    (@borrow $value:ident in $field:ident) => { $value.$field.as_ref()? };
    (@borrow $value:ident out $field:ident) => { &mut $value.$field };
    (@ $($invalid:tt)*) => { compile_error!("expected local declarations, then sequence { ... } or select { ... } with node expressions ending in ; and optional .with(local, out local) bindings") };
    (context: $context:ty; $($body:tt)*) => {
        $crate::scope::scope!(@locals [__FlatbtLocals __flatbt_locals $context] [] []; $($body)*)
    };
    ($($body:tt)*) => {
        $crate::scope::scope!(@locals [__FlatbtLocals __flatbt_locals _] [] []; $($body)*)
    };
}
