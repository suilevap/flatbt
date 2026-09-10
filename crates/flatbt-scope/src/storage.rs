use std::marker::PhantomData;

use flatbt_core::{BtNode, EntryMode, NodeResult};

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
