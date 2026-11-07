use crate::{BtControl, ControlNode, ControlOp};

/// Selects from shared blackboard on Evaluate; forwards the child's result.
/// Resume follows the saved child.
pub struct Choose<Chooser>(pub Chooser);

impl<Blackboard, Chooser: Fn(&Blackboard) -> usize> BtControl<Blackboard> for Choose<Chooser> {
    type State = ();
    type Memory = ();

    fn kind(&self) -> &'static str {
        "choose"
    }

    fn begin(
        &self,
        _: &mut (),
        _: &mut (),
        bb: &mut Blackboard,
        _: Option<usize>,
        _: usize,
    ) -> ControlOp {
        ControlOp::RunChild((self.0)(bb))
    }

    fn child_succeeded(
        &self,
        _: &mut (),
        _: &mut (),
        _: &mut Blackboard,
        _: usize,
        _: usize,
    ) -> ControlOp {
        ControlOp::Success
    }

    fn child_failed(
        &self,
        _: &mut (),
        _: &mut (),
        _: &mut Blackboard,
        _: usize,
        _: usize,
    ) -> ControlOp {
        ControlOp::Failure
    }
}

/// Static choice node, built by [`crate::choose!`].
pub type ChooseNode<Chooser, Children> = ControlNode<Choose<Chooser>, Children>;

/// Matches shared blackboard to select a statically typed node.
///
/// ```
/// use flatbt::{BtState, EntryMode, NodeResult, check, leaf, update};
/// use flatbt::choose;
///
/// let tree = choose!(|value: &usize| match *value {
///     0 => leaf(|value: &mut usize| {
///         *value = 1;
///         NodeResult::Success
///     }),
///     _ => check(|value: &usize| *value > 0),
/// });
/// let mut state: BtState<_, _> = BtState::new(&tree);
/// let mut value = 0;
/// assert_eq!(update(&tree, &mut state, &mut value, EntryMode::Resume), NodeResult::Success);
/// assert_eq!(value, 1);
/// ```
///
/// Arm definitions are built once, in order, without access to blackboard or match
/// bindings. Evaluate repeats patterns/guards; Resume follows the saved arm.
/// Returns the chosen result without fallback. Each arm has distinct identity,
/// even with the same node type; an or-pattern shares one arm.
///
/// Nested choices use static tuple dispatch and inline state. No added heap
/// allocation or type erasure. Limit: `FLATBT_MAX_CHILDREN` (default 32).
/// Use `move |blackboard: &Blackboard| match ...` to own chooser captures.
#[macro_export]
macro_rules! choose {
    ([$($indices:literal)*] $($args:tt)*) => {
        $crate::choose!(@arms [$($indices)*] $($args)*)
    };
    (|$bb:ident: $blackboard_type:ty| match $($tail:tt)+) => {
        $crate::choose!(@match [] [$bb: $blackboard_type] [] $($tail)+)
    };
    (move |$bb:ident: $blackboard_type:ty| match $($tail:tt)+) => {
        $crate::choose!(@match [move] [$bb: $blackboard_type] [] $($tail)+)
    };
    // The final brace group contains match arms; earlier tokens form the scrutinee.
    (@match [$($capture:tt)*] [$bb:ident: $blackboard_type:ty] [$($value:tt)+]
        { $($arms:tt)* } $(,)?) => {
        $crate::choose!(@parse [[$($capture)*] [$bb: $blackboard_type] [$($value)+]]
            [] ; $($arms)*)
    };
    (@match [$($capture:tt)*] [$bb:ident: $blackboard_type:ty] [$($value:tt)*]
        $next:tt $($tail:tt)*) => {
        $crate::choose!(@match [$($capture)*] [$bb: $blackboard_type]
            [$($value)* $next] $($tail)*)
    };
    // Block arms may omit commas.
    (@parse $setup:tt [$($arms:tt)*] ;
        $pattern:pat $(if $guard:expr)? => $node:block $(,)? $($rest:tt)*) => {
        $crate::choose!(@parse $setup
            [$($arms)* [$pattern $(if $guard)?] [$node]] ; $($rest)*)
    };
    (@parse $setup:tt [$($arms:tt)*] ;
        $pattern:pat $(if $guard:expr)? => $node:expr $(, $($rest:tt)*)?) => {
        $crate::choose!(@parse $setup
            [$($arms)* [$pattern $(if $guard)?] [$node]] ; $($($rest)*)?)
    };
    (@parse [$($setup:tt)*] [$($arms:tt)*] ;) => {
        $crate::__flatbt_child_indices!([$crate::choose]; $($setup)* [] [] ; $($arms)*)
    };
    (@arms [$index:literal $($indices:literal)*]
        [$($capture:tt)*] [$bb:ident: $blackboard_type:ty] [$($value:tt)+]
        [$($nodes:tt)*] [$($selection:tt)*] ;
        [$pattern:pat $(if $guard:expr)?] [$node:expr] $($rest:tt)*) => {
        $crate::choose!(@arms [$($indices)*]
            [$($capture)*] [$bb: $blackboard_type] [$($value)+]
            [$($nodes)* $crate::inspect::label(stringify!($pattern $(if $guard)?), $node),]
            [$($selection)* $pattern $(if $guard)? => $index,]
            ; $($rest)*
        )
    };
    (@arms [$($indices:literal)*]
        [$($capture:tt)*] [$bb:ident: $blackboard_type:ty] [$($value:tt)+]
        [$($nodes:tt)*] [$($selection:tt)*] ;) => {
        $crate::control(
            $crate::Choose($($capture)* |$bb: $blackboard_type| match $($value)+ { $($selection)* }),
            ($($nodes)*),
        )
    };
    (@arms [] $($rest:tt)*) => {
        compile_error!("choose! candidate count exceeds FLATBT_MAX_CHILDREN")
    };
}
