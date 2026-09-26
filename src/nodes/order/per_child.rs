/// Writes a value per child next to the child: builds the function an order
/// reads, `Fn(&C, usize) -> T`, and the children tuple, as a pair.
///
/// ```
/// use flatbt::prelude::*;
///
/// struct Guard { threat: f32, health: f32 }
///
/// let (score, options) = per_child!(|bb: &Guard| {
///     bb.threat => leaf(|_: &mut Guard| NodeResult::Running("fight")),
///     1.0 - bb.health => leaf(|_: &mut Guard| NodeResult::Running("retreat")),
///     0.2 => leaf(|_: &mut Guard| NodeResult::Running("patrol")),
/// });
/// let tree = select(order_by(by_score(score).inertia(0.1), options));
///
/// let mut state = BtState::new(&tree);
/// let mut guard = Guard { threat: 0.1, health: 0.3 };
/// assert_eq!(update(&tree, &mut state, &mut guard, EntryMode::Evaluate).act(), Some("retreat"));
/// ```
///
/// The pair suits any order that takes a per-child function: scores for
/// [`by_score`](crate::nodes::by_score), weights for
/// [`weighted`](crate::nodes::weighted). Adding or reordering an arm cannot
/// shift a value onto another child.
///
/// Each arm is `value => node`. A value is an expression over the context
/// argument, asked whenever the order reads it; node definitions are built
/// once, in arm order, without access to it. Use `move |ctx: &Context|` to own
/// captures. Inside a larger expression, a block holds the `let`:
/// `{ let (weight, idle) = per_child!(..); select(order_by(weighted(rng, weight), idle)) }`.
/// Limit: `FLATBT_MAX_CHILDREN` arms.
#[macro_export]
macro_rules! per_child {
    ([$($indices:literal)*] $($args:tt)*) => {
        $crate::per_child!(@arms [$($indices)*] $($args)*)
    };
    (|$bb:ident: $context:ty| { $($arms:tt)* } $(,)?) => {
        $crate::per_child!(@parse [[] [$bb: $context]] [] ; $($arms)*)
    };
    (move |$bb:ident: $context:ty| { $($arms:tt)* } $(,)?) => {
        $crate::per_child!(@parse [[move] [$bb: $context]] [] ; $($arms)*)
    };
    (@parse $setup:tt [$($arms:tt)*] ; $value:expr => $node:expr $(, $($rest:tt)*)?) => {
        $crate::per_child!(@parse $setup [$($arms)* [$value] [$node]] ; $($($rest)*)?)
    };
    (@parse $setup:tt [] ;) => {
        compile_error!("per_child! needs at least one `value => node` arm")
    };
    (@parse [$($setup:tt)*] [$($arms:tt)*] ;) => {
        $crate::__flatbt_child_indices!([$crate::per_child]; $($setup)* [] [] ; $($arms)*)
    };
    // The last arm takes `_`, so the match needs no unreachable arm.
    (@arms [$index:literal $($indices:literal)*]
        [$($capture:tt)*] [$bb:ident: $context:ty]
        [$($nodes:tt)*] [$($values:tt)*] ;
        [$value:expr] [$node:expr]) => {
        (
            $($capture)* |$bb: $context, index: usize| match index { $($values)* _ => $value },
            ($($nodes)* $node,),
        )
    };
    (@arms [$index:literal $($indices:literal)*]
        [$($capture:tt)*] [$bb:ident: $context:ty]
        [$($nodes:tt)*] [$($values:tt)*] ;
        [$value:expr] [$node:expr] $($rest:tt)+) => {
        $crate::per_child!(@arms [$($indices)*]
            [$($capture)*] [$bb: $context]
            [$($nodes)* $node,]
            [$($values)* $index => $value,]
            ; $($rest)+
        )
    };
    (@arms [] $($rest:tt)*) => {
        compile_error!("per_child! arm count exceeds FLATBT_MAX_CHILDREN")
    };
}
