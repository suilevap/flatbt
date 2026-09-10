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
/// use flatbt_core::{BtNode, BtState, EntryMode, NodeResult, update};
/// use flatbt_scope::scope;
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
/// use flatbt_core::{BtNode, BtState, EntryMode, NodeResult};
/// use flatbt_scope::scope;
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
#[doc(hidden)]
#[macro_export]
macro_rules! __flatbt_scope {
    (@locals [$locals:ident $value:ident $context:ty] [$($fields:tt)*] [$($init:tt)*];
        let $field:ident: $ty:ty = $callback:expr; $($rest:tt)*) => {
        $crate::scope!(@locals [$locals $value $context]
            [$($fields)* $field: ::core::option::Option<$ty>,]
            [$($init)* $crate::bind($crate::compute::<$context, $ty, _>($callback),
                $crate::write(|$value: &mut $locals| &mut $value.$field)),]; $($rest)*)
    };
    (@locals [$locals:ident $value:ident $context:ty] [$($fields:tt)*] [$($init:tt)*];
        let $field:ident: $ty:ty; $($rest:tt)*) => {
        $crate::scope!(@locals [$locals $value $context]
            [$($fields)* $field: ::core::option::Option<$ty>,] [$($init)*]; $($rest)*)
    };
    (@locals [$locals:ident $value:ident $context:ty] [$($fields:tt)*] [$($init:tt)*];
        $control:ident { $($body:tt)* } $(;)?) => {{
        #[derive(Default)]
        struct $locals { $($fields)* }
        $crate::scope::<$locals, _>($crate::scope!(@initialized [$($init)*]
            $crate::scope!(@children [$locals $value $context] $control []; $($body)*)))
    }};
    (@initialized [] $body:expr) => { $body };
    (@initialized [$($init:tt)+] $body:expr) => { $crate::__private::core::seq(($($init)+ $body,)) };
    (@children $setup:tt $control:ident [$($nodes:tt)*]; sequence { $($body:tt)* } $($rest:tt)*) => {
        $crate::scope!(@children $setup $control
            [$($nodes)* $crate::scope!(@children $setup sequence []; $($body)*),]; $($rest)*)
    };
    (@children $setup:tt $control:ident [$($nodes:tt)*]; select { $($body:tt)* } $($rest:tt)*) => {
        $crate::scope!(@children $setup $control
            [$($nodes)* $crate::scope!(@children $setup select []; $($body)*),]; $($rest)*)
    };
    (@children $setup:tt $control:ident $nodes:tt; ; $($rest:tt)*) => {
        $crate::scope!(@children $setup $control $nodes; $($rest)*)
    };
    (@children [$locals:ident $value:ident $context:ty] sequence [$($nodes:tt)*];) => { $crate::__private::core::seq(($($nodes)*)) };
    (@children [$locals:ident $value:ident $context:ty] select [$($nodes:tt)*];) => { $crate::__private::core::select(($($nodes)*)) };
    (@children $setup:tt $control:ident $nodes:tt; $($rest:tt)+) => {
        $crate::scope!(@expression $setup $control $nodes []; $($rest)+)
    };
    // Recognize only a final .with(...) suffix. Everything before it remains an
    // ordinary Rust expression; nested calls, closures, and blocks stay opaque.
    (@expression $setup:tt $control:ident [$($nodes:tt)*] [$($node:tt)+];
        . with ($($args:tt)*) ; $($rest:tt)*) => {
        $crate::scope!(@children $setup $control
            [$($nodes)* $crate::scope!(@args $setup [$($node)+] []; $($args)*),]; $($rest)*)
    };
    (@expression $setup:tt $control:ident [$($nodes:tt)*] [$($node:tt)+]; ; $($rest:tt)*) => {
        $crate::scope!(@children $setup $control
            [$($nodes)* $crate::no_params($($node)+),]; $($rest)*)
    };
    (@expression $setup:tt $control:ident $nodes:tt [$($node:tt)*]; $next:tt $($rest:tt)*) => {
        $crate::scope!(@expression $setup $control $nodes [$($node)* $next]; $($rest)*)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; out $field:ident $(, $($rest:tt)*)?) => {
        $crate::scope!(@args $setup $node [$($args)* out $field,]; $($($rest)*)?)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; in $field:ident $(, $($rest:tt)*)?) => {
        $crate::scope!(@args $setup $node [$($args)* in $field,]; $($($rest)*)?)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; $field:ident $(, $($rest:tt)*)?) => {
        $crate::scope!(@args $setup $node [$($args)* in $field,]; $($($rest)*)?)
    };
    (@args $setup:tt [$node:expr] [];) => { $crate::no_params($node) };
    (@args $setup:tt [$node:expr] [$($access:ident $field:ident,)+];) => {
        $crate::scope!(@bind $setup $node; $($access $field),+)
    };
    (@bind [$locals:ident $value:ident $context:ty] $node:expr; in $field:ident) => {
        $crate::bind($node, $crate::read(|$value: &$locals| $value.$field.as_ref()))
    };
    (@bind [$locals:ident $value:ident $context:ty] $node:expr; out $field:ident) => {
        $crate::bind($node, $crate::write(|$value: &mut $locals| &mut $value.$field))
    };
    (@bind [$locals:ident $value:ident $context:ty] $node:expr; $($access:ident $field:ident),+) => {
        $crate::bind($node, $crate::params::<($($crate::scope!(@shape $access),)+), _, _>(
            |$value: &mut $locals| ::core::option::Option::Some(($($crate::scope!(@borrow $value $access $field),)+))
        ))
    };
    (@shape in) => { $crate::Read<_> };
    (@shape out) => { $crate::Write<_> };
    (@borrow $value:ident in $field:ident) => { $value.$field.as_ref()? };
    (@borrow $value:ident out $field:ident) => { &mut $value.$field };
    (@ $($invalid:tt)*) => { compile_error!("expected local declarations, then sequence { ... } or select { ... } with node expressions ending in ; and optional .with(local, out local) bindings") };
    (context: $context:ty; $($body:tt)*) => {
        $crate::scope!(@locals [__FlatbtLocals __flatbt_locals $context] [] []; $($body)*)
    };
    ($($body:tt)*) => {
        $crate::scope!(@locals [__FlatbtLocals __flatbt_locals _] [] []; $($body)*)
    };
}
