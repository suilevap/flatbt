use crate::inspect::{Inspector, NodeInfo};
use crate::{BtNode, Entry, NodeResult};

/// Owns invocation-local data outside application context.
pub struct Scope<L, N> {
    child: N,
    inspect_locals: fn(&L, &mut dyn Inspector),
}

/// Initializes locals with Default on entry. Bindings select fields explicitly.
/// Producers may fill slots for later consumers. Nested scopes inherit no parameters.
pub fn scope<L, N>(child: N) -> Scope<L, N> {
    Scope {
        child,
        inspect_locals: |_, _| {},
    }
}

impl<L, N> Scope<L, N> {
    /// Sets how debug views report the locals while the scope runs, as
    /// [`Inspector::field`]s. `scope!` reports each local by name.
    pub fn inspect_locals(self, inspect: fn(&L, &mut dyn Inspector)) -> Self {
        Self {
            inspect_locals: inspect,
            ..self
        }
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
    const NODES: usize = 1 + <N as BtNode<C, A, &'static mut L>>::NODES;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        _: P,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        let entry = entry.child(1);
        let result = BtNode::<C, A, &mut L>::update(
            &self.child,
            &mut state.child,
            ctx,
            &mut state.locals,
            entry,
        );
        entry.finish(&result);
        result
    }

    fn inspect(&self, state: Option<&Self::State>, inspector: &mut dyn Inspector) {
        inspector.node(NodeInfo::new("scope", state.is_some()), |inspector| {
            if let Some(state) = state {
                (self.inspect_locals)(&state.locals, inspector);
            }
            let child = state.map(|state| &state.child);
            BtNode::<C, A, &mut L>::inspect(&self.child, child, inspector);
        });
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
        _: Entry<'_>,
    ) -> NodeResult<A> {
        *output = Some((self.0)(ctx));
        NodeResult::Success
    }

    fn inspect(&self, state: Option<&()>, inspector: &mut dyn Inspector) {
        inspector.node(NodeInfo::new("compute", state.is_some()), |_| {});
    }
}
