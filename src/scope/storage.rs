use crate::inspect::{Inspector, NodeInfo};
use crate::params::Write;
use crate::{BtChildren, BtNode, ControlOp, Entry, NodeResult};

/// Owns invocation-local data outside application context.
pub struct Scope<L, N, I = NoInit> {
    child: N,
    init: I,
    inspect_locals: fn(&L, &mut dyn Inspector),
}

/// Initializes locals with Default on entry. Bindings select fields explicitly.
/// Producers may fill slots for later consumers. Nested scopes inherit no parameters.
pub fn scope<L, N>(child: N) -> Scope<L, N> {
    Scope {
        child,
        init: NoInit,
        inspect_locals: |_, _| {},
    }
}

impl<L, N> Scope<L, N> {
    /// Runs `init`, a tuple of nodes over the locals, in order when an
    /// invocation starts, before the child; Resume and Evaluate skip them.
    /// Each must succeed on entry: one that fails fails the scope, and one
    /// still running reports a diagnostic and fails it. `scope!` puts its
    /// `let` initializers here.
    pub fn init<T>(self, init: T) -> Scope<L, N, Init<T>> {
        Scope {
            child: self.child,
            init: Init(init),
            inspect_locals: self.inspect_locals,
        }
    }
}

impl<L, N, I> Scope<L, N, I> {
    /// Sets how debug views report the locals while the scope runs, as
    /// [`Inspector::field`]s. `scope!` reports each local by name.
    pub fn inspect_locals(self, inspect: fn(&L, &mut dyn Inspector)) -> Self {
        Self {
            inspect_locals: inspect,
            ..self
        }
    }
}

/// What a [`Scope`] runs when an invocation starts: [`NoInit`], or the
/// initializers given to [`Scope::init`].
pub trait ScopeInit<C, A, L> {
    /// Invocation state; for initializers, whether they ran.
    type State: Default + Send + 'static;
    /// Nodes, numbered after the scope and before its child.
    const NODES: usize;

    /// Runs the initializers if this invocation has not: `Some` with the
    /// scope's result when one did not succeed.
    fn run(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        locals: &mut L,
        entry: Entry<'_>,
    ) -> Option<NodeResult<A>>;

    fn inspect(&self, state: Option<&Self::State>, inspector: &mut dyn Inspector);
}

/// No initializers: a scope that only owns its locals.
pub struct NoInit;

impl<C, A, L> ScopeInit<C, A, L> for NoInit {
    type State = ();
    const NODES: usize = 0;

    #[inline(always)]
    fn run(&self, _: &mut (), _: &mut C, _: &mut L, _: Entry<'_>) -> Option<NodeResult<A>> {
        None
    }

    fn inspect(&self, _: Option<&()>, _: &mut dyn Inspector) {}
}

/// Initializers, from [`Scope::init`]: a tuple of nodes over the locals.
pub struct Init<T>(T);

/// Initializer state, and whether they ran for this invocation.
#[derive(Default)]
pub struct InitState<S> {
    init: S,
    done: bool,
}

impl<C, A, L: 'static, T: BtChildren<C, A, Write<L>>> ScopeInit<C, A, L> for Init<T> {
    type State = InitState<T::State>;
    const NODES: usize = T::NODES;

    #[inline]
    fn run(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        mut locals: &mut L,
        entry: Entry<'_>,
    ) -> Option<NodeResult<A>> {
        if state.done {
            return None;
        }
        let mut next = |_: &mut C, index: usize, succeeded: bool| {
            if succeeded {
                ControlOp::RunChild(index + 1)
            } else {
                ControlOp::Failure
            }
        };
        // Initializers are numbered from the scope, like a control's children.
        match self
            .0
            .run_from(&mut state.init, 0, ctx, &mut locals, entry, &mut next)
        {
            Err(ControlOp::RunChild(done)) if done == T::LEN => {
                state.done = true;
                None
            }
            Ok(_) => Some(NodeResult::error(
                "scope initializer did not complete on entry",
            )),
            Err(_) => Some(NodeResult::Failure),
        }
    }

    fn inspect(&self, state: Option<&Self::State>, inspector: &mut dyn Inspector) {
        self.0
            .inspect_children(state.map(|state| &state.init), inspector);
    }
}

/// Inline state; descendants drop before locals.
#[derive(Default)]
pub struct ScopeState<L, S, I = ()> {
    child: S,
    init: I,
    locals: L,
}

impl<C, A, P, L, N, S, I> BtNode<C, A, P> for Scope<L, N, I>
where
    L: Default + Send + 'static,
    N: for<'a> BtNode<C, A, &'a mut L, State = S>,
    S: Default + Send + 'static,
    I: ScopeInit<C, A, L>,
{
    type State = ScopeState<L, S, I::State>;
    const NODES: usize = 1 + I::NODES + <N as BtNode<C, A, &'static mut L>>::NODES;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        _: P,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        if let Some(result) = self
            .init
            .run(&mut state.init, ctx, &mut state.locals, entry)
        {
            return result;
        }
        entry.run(
            1 + I::NODES,
            &self.child,
            &mut state.child,
            ctx,
            &mut state.locals,
        )
    }

    fn inspect(&self, state: Option<&Self::State>, inspector: &mut dyn Inspector) {
        inspector.node(NodeInfo::new("scope", state.is_some()), |inspector| {
            if let Some(state) = state {
                (self.inspect_locals)(&state.locals, inspector);
            }
            self.init.inspect(state.map(|state| &state.init), inspector);
            let child = state.map(|state| &state.child);
            BtNode::<C, A, &mut L>::inspect(&self.child, child, inspector);
        });
    }
}

/// Synchronous output node, called on entry. `scope!` makes each `let`
/// initializer one, run by [`Scope::init`].
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
