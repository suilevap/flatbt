/// Owns named locals with explicit sequence/select control flow.
///
/// - `let name: Type = callback;`: initialize with `Fn(&mut Context) -> Type`.
/// - `let name: Type;`: reserve an output slot for a producer.
/// - `Node.with(name);`: shared input. `out name`: exclusive output slot.
/// - Plain node expressions use unit parameters; constructors take ordinary Rust
///   arguments. Only `.with(...)` binds runtime fields.
///   Annotate the initializer's argument. Nested sequence/select blocks share
///   locals.
///
/// Definitions are built once. Initializers run in declaration order before the
/// body, once per invocation; Resume/Evaluate preserve them while Running.
/// Missing inputs fail at runtime; Rust rejects aliasing writes.
/// Parameter tuple limit: `FLATBT_MAX_PARAMS`.
///
/// ```
/// use flatbt::{BtNode, BtState, EntryMode, NodeResult, update};
/// use flatbt::scope::scope;
/// struct Observe;
/// impl BtNode<Vec<u32>, (), &u32> for Observe {
///     type State = ();
///     fn update(&self, _: &mut (), ctx: &mut Vec<u32>, input: &u32, _: EntryMode) -> NodeResult {
///         ctx.push(*input);
///         NodeResult::Success
///     }
/// }
/// let tree = scope! {
///     let walk_pos: u32 = |_: &mut Vec<u32>| 10;
///     let door_pos: u32 = |_: &mut Vec<u32>| 90;
///     sequence {
///         Observe.with(door_pos);
///         Observe.with(walk_pos);
///     }
/// };
/// let mut state: BtState<_, _> = BtState::new(&tree);
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
/// impl BtNode<(), (), (&mut Option<u32>, &mut Option<u32>)> for TwoOutputs {
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
    (@locals [$locals:ident $value:ident] [$($fields:tt)*] [$($init:tt)*];
        let $field:ident: $ty:ty = $callback:expr; $($rest:tt)*) => {
        $crate::__flatbt_scope!(@locals [$locals $value]
            [$($fields)* $field: ::core::option::Option<$ty>,]
            [$($init)* $crate::scope::bind($crate::scope::compute($callback),
                $crate::scope::write(|$value: &mut $locals| &mut $value.$field)),]; $($rest)*)
    };
    (@locals [$locals:ident $value:ident] [$($fields:tt)*] [$($init:tt)*];
        let $field:ident: $ty:ty; $($rest:tt)*) => {
        $crate::__flatbt_scope!(@locals [$locals $value]
            [$($fields)* $field: ::core::option::Option<$ty>,] [$($init)*]; $($rest)*)
    };
    (@locals [$locals:ident $value:ident] [$($fields:tt)*] [$($init:tt)*];
        $control:ident { $($body:tt)* } $(;)?) => {{
        #[derive(Default)]
        struct $locals { $($fields)* }
        $crate::scope::scope::<$locals, _>($crate::__flatbt_scope!(@initialized [$($init)*]
            $crate::__flatbt_scope!(@children [$locals $value] $control []; $($body)*)))
    }};
    (@initialized [] $body:expr) => { $body };
    (@initialized [$($init:tt)+] $body:expr) => { $crate::seq(($($init)+ $body,)) };
    (@children $setup:tt $control:ident [$($nodes:tt)*]; sequence { $($body:tt)* } $($rest:tt)*) => {
        $crate::__flatbt_scope!(@children $setup $control
            [$($nodes)* $crate::__flatbt_scope!(@children $setup sequence []; $($body)*),]; $($rest)*)
    };
    (@children $setup:tt $control:ident [$($nodes:tt)*]; select { $($body:tt)* } $($rest:tt)*) => {
        $crate::__flatbt_scope!(@children $setup $control
            [$($nodes)* $crate::__flatbt_scope!(@children $setup select []; $($body)*),]; $($rest)*)
    };
    (@children $setup:tt $control:ident $nodes:tt; ; $($rest:tt)*) => {
        $crate::__flatbt_scope!(@children $setup $control $nodes; $($rest)*)
    };
    (@children [$locals:ident $value:ident] sequence [$($nodes:tt)*];) => { $crate::seq(($($nodes)*)) };
    (@children [$locals:ident $value:ident] select [$($nodes:tt)*];) => { $crate::select(($($nodes)*)) };
    (@children $setup:tt $control:ident $nodes:tt; $($rest:tt)+) => {
        $crate::__flatbt_scope!(@expression $setup $control $nodes []; $($rest)+)
    };
    // Only the final .with(...) binds locals; preceding Rust expressions stay opaque.
    (@expression $setup:tt $control:ident [$($nodes:tt)*] [$($node:tt)+];
        . with ($($args:tt)*) ; $($rest:tt)*) => {
        $crate::__flatbt_scope!(@children $setup $control
            [$($nodes)* $crate::__flatbt_scope!(@args $setup [$($node)+] []; $($args)*),]; $($rest)*)
    };
    (@expression $setup:tt $control:ident [$($nodes:tt)*] [$($node:tt)+]; ; $($rest:tt)*) => {
        $crate::__flatbt_scope!(@children $setup $control
            [$($nodes)* $crate::scope::no_params($($node)+),]; $($rest)*)
    };
    (@expression $setup:tt $control:ident $nodes:tt [$($node:tt)*]; $next:tt $($rest:tt)*) => {
        $crate::__flatbt_scope!(@expression $setup $control $nodes [$($node)* $next]; $($rest)*)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; out $field:ident $(, $($rest:tt)*)?) => {
        $crate::__flatbt_scope!(@args $setup $node [$($args)* out $field,]; $($($rest)*)?)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; in $field:ident $(, $($rest:tt)*)?) => {
        $crate::__flatbt_scope!(@args $setup $node [$($args)* in $field,]; $($($rest)*)?)
    };
    (@args $setup:tt $node:tt [$($args:tt)*]; $field:ident $(, $($rest:tt)*)?) => {
        $crate::__flatbt_scope!(@args $setup $node [$($args)* in $field,]; $($($rest)*)?)
    };
    (@args $setup:tt [$node:expr] [];) => { $crate::scope::no_params($node) };
    (@args $setup:tt [$node:expr] [$($access:ident $field:ident,)+];) => {
        $crate::__flatbt_scope!(@bind $setup $node; $($access $field),+)
    };
    (@bind [$locals:ident $value:ident] $node:expr; in $field:ident) => {
        $crate::scope::bind($node, $crate::scope::read(|$value: &$locals| $value.$field.as_ref()))
    };
    (@bind [$locals:ident $value:ident] $node:expr; out $field:ident) => {
        $crate::scope::bind($node, $crate::scope::write(|$value: &mut $locals| &mut $value.$field))
    };
    (@bind [$locals:ident $value:ident] $node:expr; $($access:ident $field:ident),+) => {
        $crate::scope::bind($node, $crate::scope::params::<($($crate::__flatbt_scope!(@shape $access),)+), _, _>(
            |$value: &mut $locals| ::core::option::Option::Some(($($crate::__flatbt_scope!(@borrow $value $access $field),)+))
        ))
    };
    (@shape in) => { $crate::params::Read<_> };
    (@shape out) => { $crate::params::Write<_> };
    (@borrow $value:ident in $field:ident) => { $value.$field.as_ref()? };
    (@borrow $value:ident out $field:ident) => { &mut $value.$field };
    (@ $($invalid:tt)*) => { compile_error!("expected local declarations, then sequence { ... } or select { ... } with node expressions ending in ; and optional .with(local, out local) bindings") };
    ($($body:tt)*) => {
        $crate::__flatbt_scope!(@locals [__FlatbtLocals __flatbt_locals] [] []; $($body)*)
    };
}
