use crate::inspect::{Inspector, NodeInfo};
use crate::params::Write;
use crate::{BtChildren, BtNode, ControlOp, Entry, NodeResult};

/// Owns invocation-local data outside application context.
pub struct Scope<Locals, Child, Initializers = NoInit> {
    child: Child,
    init: Initializers,
    inspect_locals: fn(&Locals, &mut dyn Inspector),
}

/// Initializes locals with Default on entry. Bindings select fields explicitly.
/// Producers may fill slots for later consumers. Nested scopes inherit no parameters.
pub fn scope<Locals, Child>(child: Child) -> Scope<Locals, Child> {
    Scope {
        child,
        init: NoInit,
        inspect_locals: |_, _| {},
    }
}

impl<Locals, Child> Scope<Locals, Child> {
    /// Runs `init`, a tuple of nodes over the locals, in order when an
    /// invocation starts, before the child; Resume and Evaluate skip them.
    /// Each must succeed on entry: one that fails fails the scope, and one
    /// still running reports a diagnostic and fails it. `scope!` puts its
    /// `let` initializers here.
    pub fn init<Nodes>(self, init: Nodes) -> Scope<Locals, Child, Init<Nodes>> {
        Scope {
            child: self.child,
            init: Init(init),
            inspect_locals: self.inspect_locals,
        }
    }
}

impl<Locals, Child, Initializers> Scope<Locals, Child, Initializers> {
    /// Sets how debug views report the locals while the scope runs, as
    /// [`Inspector::field`]s. `scope!` reports each local by name.
    pub fn inspect_locals(self, inspect: fn(&Locals, &mut dyn Inspector)) -> Self {
        Self {
            inspect_locals: inspect,
            ..self
        }
    }
}

/// What a [`Scope`] runs when an invocation starts: [`NoInit`], or the
/// initializers given to [`Scope::init`].
pub trait ScopeInit<Context, Act, Locals> {
    /// Invocation state; for initializers, whether they ran.
    type State: Default + Send + 'static;
    /// The initializers' memory.
    type Memory: Default + Send + 'static;
    /// Nodes, numbered after the scope and before its child.
    const NODES: usize;

    /// Runs the initializers if this invocation has not: `Some` with the
    /// scope's result when one did not succeed.
    fn run(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut Context,
        locals: &mut Locals,
        entry: Entry<'_>,
    ) -> Option<NodeResult<Act>>;

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &Self::Memory,
        inspector: &mut dyn Inspector,
    );
}

/// No initializers: a scope that only owns its locals.
pub struct NoInit;

impl<Context, Act, Locals> ScopeInit<Context, Act, Locals> for NoInit {
    type State = ();
    type Memory = ();
    const NODES: usize = 0;

    #[inline(always)]
    fn run(
        &self,
        _: &mut (),
        _: &mut (),
        _: &mut Context,
        _: &mut Locals,
        _: Entry<'_>,
    ) -> Option<NodeResult<Act>> {
        None
    }

    fn inspect(&self, _: Option<&()>, _: &(), _: &mut dyn Inspector) {}
}

/// Initializers, from [`Scope::init`]: a tuple of nodes over the locals.
pub struct Init<Nodes>(Nodes);

/// Initializer state, and whether they ran for this invocation.
#[derive(Default)]
pub struct InitState<NodesState> {
    init: NodesState,
    done: bool,
}

impl<Context, Act, Locals: 'static, Nodes: BtChildren<Context, Act, Write<Locals>>>
    ScopeInit<Context, Act, Locals> for Init<Nodes>
{
    type State = InitState<Nodes::State>;
    type Memory = Nodes::Memory;
    const NODES: usize = Nodes::NODES;

    #[inline]
    fn run(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut Context,
        mut locals: &mut Locals,
        entry: Entry<'_>,
    ) -> Option<NodeResult<Act>> {
        if state.done {
            return None;
        }
        let mut next = |_: &mut Context, index: usize, succeeded: bool| {
            if succeeded {
                ControlOp::RunChild(index + 1)
            } else {
                ControlOp::Failure
            }
        };
        // Initializers are numbered from the scope, like a control's children.
        match self.0.run_from(
            &mut state.init,
            memory,
            0,
            ctx,
            &mut locals,
            entry,
            &mut next,
        ) {
            Err(ControlOp::RunChild(done)) if done == Nodes::LEN => {
                state.done = true;
                None
            }
            Ok(_) => Some(entry.error("scope initializer did not complete on entry")),
            Err(_) => Some(NodeResult::Failure),
        }
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &Self::Memory,
        inspector: &mut dyn Inspector,
    ) {
        self.0
            .inspect_children(state.map(|state| &state.init), memory, inspector);
    }
}

/// Memory of a scope's child and initializers.
#[derive(Default)]
pub struct ScopeMemory<ChildMemory, InitializersMemory = ()> {
    child: ChildMemory,
    init: InitializersMemory,
}

/// Inline state; descendants drop before locals.
#[derive(Default)]
pub struct ScopeState<Locals, ChildState, InitializersState = ()> {
    child: ChildState,
    init: InitializersState,
    locals: Locals,
}

impl<Context, Act, Params, Locals, Child, ChildState, ChildMemory, Initializers>
    BtNode<Context, Act, Params> for Scope<Locals, Child, Initializers>
where
    Locals: Default + Send + 'static,
    Child: for<'a> BtNode<Context, Act, &'a mut Locals, State = ChildState, Memory = ChildMemory>,
    ChildState: Default + Send + 'static,
    ChildMemory: Default + Send + 'static,
    Initializers: ScopeInit<Context, Act, Locals>,
{
    type State = ScopeState<Locals, ChildState, Initializers::State>;
    type Memory = ScopeMemory<ChildMemory, Initializers::Memory>;
    const NODES: usize =
        1 + Initializers::NODES + <Child as BtNode<Context, Act, &'static mut Locals>>::NODES;

    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut Context,
        _: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        if let Some(result) = self.init.run(
            &mut state.init,
            &mut memory.init,
            ctx,
            &mut state.locals,
            entry,
        ) {
            return result;
        }
        entry.run(
            1 + Initializers::NODES,
            &self.child,
            &mut state.child,
            &mut memory.child,
            ctx,
            &mut state.locals,
        )
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &Self::Memory,
        inspector: &mut dyn Inspector,
    ) {
        inspector.node(NodeInfo::new("scope", state.is_some()), |inspector| {
            if let Some(state) = state {
                (self.inspect_locals)(&state.locals, inspector);
            }
            self.init
                .inspect(state.map(|state| &state.init), &memory.init, inspector);
            let child = state.map(|state| &state.child);
            BtNode::<Context, Act, &mut Locals>::inspect(
                &self.child,
                child,
                &memory.child,
                inspector,
            );
        });
    }
}

/// Synchronous output node, called on entry. `scope!` makes each `let`
/// initializer one, run by [`Scope::init`].
pub struct Compute<Function>(Function);

/// Computes from context and fills the output slot.
/// The callable is checked where the tree runs, like [`crate::leaf`], so
/// an initializer closure stays open to inference. Annotate its argument.
pub fn compute<Function>(init: Function) -> Compute<Function> {
    Compute(init)
}

impl<Context, Act, Value, Function: Fn(&mut Context) -> Value>
    BtNode<Context, Act, &mut Option<Value>> for Compute<Function>
{
    type State = ();
    type Memory = ();

    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        ctx: &mut Context,
        output: &mut Option<Value>,
        _: Entry<'_>,
    ) -> NodeResult<Act> {
        *output = Some((self.0)(ctx));
        NodeResult::Success
    }

    fn inspect(&self, state: Option<&()>, _: &(), inspector: &mut dyn Inspector) {
        inspector.node(NodeInfo::new("compute", state.is_some()), |_| {});
    }
}
