use flatbt_core::{BtControl, BtNode, ControlNode, ControlOp, EntryMode, NodeResult, control};

/// Selects from shared context on Evaluate; forwards the child's result.
/// Resume follows the saved child.
pub struct Choose<F>(pub F);

impl<C, F: Fn(&C) -> usize> BtControl<C> for Choose<F> {
    type State = ();

    fn begin(&self, _: &mut (), ctx: &mut C, _: Option<usize>, _: usize) -> ControlOp {
        ControlOp::RunChild((self.0)(ctx))
    }

    fn child_succeeded(&self, _: &mut (), _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Success
    }

    fn child_failed(&self, _: &mut (), _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Failure
    }
}

/// Static choice node. Use [`crate::choose!`] to generate candidate indices.
pub struct ChooseNode<F, Children>(ControlNode<Choose<F>, Children>);

impl<F, Children> ChooseNode<F, Children> {
    /// Chooser returns a tuple index. [`crate::choose!`] generates these indices.
    pub fn new(children: Children, choose: F) -> Self {
        Self(control(Choose(choose), children))
    }
}

// A local wrapper permits an inherent constructor across the crate boundary.
impl<C, A, P, F, Children> BtNode<C, A, P> for ChooseNode<F, Children>
where
    ControlNode<Choose<F>, Children>: BtNode<C, A, P>,
{
    type State = <ControlNode<Choose<F>, Children> as BtNode<C, A, P>>::State;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult<A> {
        self.0.update(state, ctx, params, mode)
    }
}

/// Matches shared context to select a statically typed node.
///
/// ```
/// use flatbt_core::{BtState, EntryMode, NodeResult, check, leaf, update};
/// use flatbt_nodes::choose;
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
/// Arm definitions are built once, in order, without access to context or match
/// bindings. Evaluate repeats patterns/guards; Resume follows the saved arm.
/// Returns the chosen result without fallback. Each arm has distinct identity,
/// even with the same node type; an or-pattern shares one arm.
///
/// Nested choices use static tuple dispatch and inline state. No added heap
/// allocation or type erasure. Limit: `FLATBT_MAX_CHILDREN` (default 32).
/// Use `move |context: &Context| match ...` to own chooser captures.
#[macro_export]
macro_rules! choose {
    ([$($indices:literal)*] $($args:tt)*) => {
        $crate::choose!(@arms [$($indices)*] $($args)*)
    };
    (|$bb:ident: $context:ty| match $($tail:tt)+) => {
        $crate::choose!(@match [] [$bb: $context] [] $($tail)+)
    };
    (move |$bb:ident: $context:ty| match $($tail:tt)+) => {
        $crate::choose!(@match [move] [$bb: $context] [] $($tail)+)
    };
    // The final brace group contains match arms; earlier tokens form the scrutinee.
    (@match [$($capture:tt)*] [$bb:ident: $context:ty] [$($value:tt)+]
        { $($arms:tt)* } $(,)?) => {
        $crate::choose!(@parse [[$($capture)*] [$bb: $context] [$($value)+]]
            [] ; $($arms)*)
    };
    (@match [$($capture:tt)*] [$bb:ident: $context:ty] [$($value:tt)*]
        $next:tt $($tail:tt)*) => {
        $crate::choose!(@match [$($capture)*] [$bb: $context]
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
        $crate::__private::core::__flatbt_child_indices!([$crate::choose]; $($setup)* [] [] ; $($arms)*)
    };
    (@arms [$index:literal $($indices:literal)*]
        [$($capture:tt)*] [$bb:ident: $context:ty] [$($value:tt)+]
        [$($nodes:tt)*] [$($selection:tt)*] ;
        [$pattern:pat $(if $guard:expr)?] [$node:expr] $($rest:tt)*) => {
        $crate::choose!(@arms [$($indices)*]
            [$($capture)*] [$bb: $context] [$($value)+]
            [$($nodes)* $node,]
            [$($selection)* $pattern $(if $guard)? => $index,]
            ; $($rest)*
        )
    };
    (@arms [$($indices:literal)*]
        [$($capture:tt)*] [$bb:ident: $context:ty] [$($value:tt)+]
        [$($nodes:tt)*] [$($selection:tt)*] ;) => {
        $crate::ChooseNode::new(
            ($($nodes)*),
            $($capture)* |$bb: $context| match $($value)+ { $($selection)* },
        )
    };
    (@arms [] $($rest:tt)*) => {
        compile_error!("choose! candidate count exceeds FLATBT_MAX_CHILDREN")
    };
}
