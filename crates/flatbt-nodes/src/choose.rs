use flatbt_core::{BtControl, BtNode, ControlNode, ControlOp, EntryMode, NodeResult, control};

/// Chooses one child from shared application context and forwards its result.
/// Resume follows the saved child through the ordinary control protocol.
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

/// Selects one statically known candidate from the application context.
/// Construct with [`crate::choose!`] to generate candidate indices automatically.
pub struct ChooseNode<F, Children>(ControlNode<Choose<F>, Children>);

impl<F, Children> ChooseNode<F, Children> {
    /// Low-level constructor. The chooser returns a tuple child index.
    /// Prefer [`crate::choose!`] for an exhaustive match with no manual indices.
    pub fn new(children: Children, choose: F) -> Self {
        Self(control(Choose(choose), children))
    }
}

// The wrapper keeps the constructor local to this crate and reuses core execution.
impl<C, P, F, Children> BtNode<C, P> for ChooseNode<F, Children>
where
    ControlNode<Choose<F>, Children>: BtNode<C, P>,
{
    type State = <ControlNode<Choose<F>, Children> as BtNode<C, P>>::State;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult {
        self.0.update(state, ctx, params, mode)
    }
}

/// Selects among concrete node definitions using a match on shared context.
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
/// let mut state = BtState::new(&tree);
/// let mut value = 0;
/// assert_eq!(update(&tree, &mut state, &mut value, EntryMode::Resume), NodeResult::Success);
/// assert_eq!(value, 1);
/// ```
///
/// Every arm's node expression is evaluated once, in source order, when building
/// the tree. Those expressions cannot use the context argument or match bindings.
/// Patterns and guards execute on Evaluate; Resume follows the saved candidate.
/// The chosen child's result is returned directly, without trying other arms.
/// Each arm is a separate candidate even when two arms have the same node type.
///
/// Candidates use the existing tuple child enum and static dispatch, including
/// when nesting this macro. No type erasure or runtime heap allocation is added.
/// The candidate count shares the FLATBT_MAX_CHILDREN tuple limit (default 32).
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
    // Collect the scrutinee until the final brace group, which contains the arms.
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
    // Rust (and rustfmt) allows block arms to omit the separating comma.
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
