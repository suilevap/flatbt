use core::marker::PhantomData;

use crate::{BtNode, EntryMode, NodeResult};

/// Owns invocation-local data outside application context.
pub struct Scope<L, N> {
    child: N,
    locals: PhantomData<fn() -> L>,
}

/// Initializes locals with Default on entry. Bindings select fields explicitly.
/// Producers may fill slots for later consumers. Nested scopes inherit no parameters.
pub fn scope<L, N>(child: N) -> Scope<L, N> {
    Scope {
        child,
        locals: PhantomData,
    }
}

/// Inline state; descendants drop before locals.
#[derive(Default)]
pub struct ScopeState<L, S> {
    child: S,
    locals: L,
}

impl<C, A, P, L, N, S> BtNode<C, A, P> for Scope<L, N>
where
    L: Default + Send + 'static,
    N: for<'a> BtNode<C, A, &'a mut L, State = S>,
    S: Default + Send + 'static,
{
    type State = ScopeState<L, S>;

    fn update(&self, state: &mut Self::State, ctx: &mut C, _: P, mode: EntryMode) -> NodeResult<A> {
        BtNode::<C, A, &mut L>::update(&self.child, &mut state.child, ctx, &mut state.locals, mode)
    }
}

/// Synchronous output node, called on entry. `scope!` places initializers before
/// the body control and skips them on Resume/Evaluate while Running.
pub struct Compute<F>(F);

/// Computes from context and fills the output slot.
/// The callable is checked where the tree runs, like [`crate::leaf`], so
/// an initializer closure stays open to inference. Annotate its argument.
pub fn compute<F>(init: F) -> Compute<F> {
    Compute(init)
}

impl<C, A, T, F: Fn(&mut C) -> T> BtNode<C, A, &mut Option<T>> for Compute<F> {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        ctx: &mut C,
        output: &mut Option<T>,
        _: EntryMode,
    ) -> NodeResult<A> {
        *output = Some((self.0)(ctx));
        NodeResult::Success
    }
}
