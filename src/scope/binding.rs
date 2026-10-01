use core::marker::PhantomData;

use crate::inspect::Inspector;
use crate::params::ParamShape;
use crate::{BtNode, Entry, NodeResult};

/// Projects local fields into borrowed node parameters.
pub trait ParamBinding<Locals> {
    type Params<'a>
    where
        Locals: 'a;

    fn get<'a>(&self, locals: &'a mut Locals) -> Option<Self::Params<'a>>;
}

pub struct ReadBinding<Project>(Project);

/// Binds a shared input. Return None for a missing value.
pub fn read<Locals, Target, Project>(project: Project) -> ReadBinding<Project>
where
    Project: Fn(&Locals) -> Option<&Target>,
{
    ReadBinding(project)
}

impl<Locals, Target: 'static, Project> ParamBinding<Locals> for ReadBinding<Project>
where
    Project: Fn(&Locals) -> Option<&Target>,
{
    type Params<'a>
        = &'a Target
    where
        Locals: 'a;

    fn get<'a>(&self, locals: &'a mut Locals) -> Option<&'a Target> {
        (self.0)(locals)
    }
}

pub struct WriteBinding<Project>(Project);

/// Binds an exclusive parameter, usually an `Option<Target>` output slot.
pub fn write<Locals, Target, Project>(project: Project) -> WriteBinding<Project>
where
    Project: Fn(&mut Locals) -> &mut Target,
{
    WriteBinding(project)
}

impl<Locals, Target: 'static, Project> ParamBinding<Locals> for WriteBinding<Project>
where
    Project: Fn(&mut Locals) -> &mut Target,
{
    type Params<'a>
        = &'a mut Target
    where
        Locals: 'a;

    fn get<'a>(&self, locals: &'a mut Locals) -> Option<&'a mut Target> {
        Some((self.0)(locals))
    }
}

pub struct ParamsBinding<Shape, Project> {
    project: Project,
    shape: PhantomData<fn() -> Shape>,
}

/// Binds multiple parameters with one projection; Rust checks disjoint writes.
/// Shape example: `(Read<Enemy>, Write<Option<Position>>)`. Custom [`ParamShape`]
/// implementations can use named fields.
pub fn params<Shape, Locals, Project>(project: Project) -> ParamsBinding<Shape, Project>
where
    Shape: ParamShape,
    Project: for<'a> Fn(&'a mut Locals) -> Option<Shape::Value<'a>>,
{
    ParamsBinding {
        project,
        shape: PhantomData,
    }
}

impl<Locals, Shape, Project> ParamBinding<Locals> for ParamsBinding<Shape, Project>
where
    Shape: ParamShape,
    Project: for<'a> Fn(&'a mut Locals) -> Option<Shape::Value<'a>>,
{
    type Params<'a>
        = Shape::Value<'a>
    where
        Locals: 'a;

    fn get<'a>(&self, locals: &'a mut Locals) -> Option<Self::Params<'a>> {
        (self.project)(locals)
    }
}

pub struct Bound<Node, Binding> {
    node: Node,
    binding: Binding,
}

/// Binds local fields to node parameters on each update; context and state stay
/// unchanged. Missing inputs log a diagnostic and fail without calling the node.
/// Output writes survive Failure. State cannot retain parameter borrows.
pub fn bind<Node, Binding>(node: Node, binding: Binding) -> Bound<Node, Binding> {
    Bound { node, binding }
}

/// Adds `node.with(binding)`, equivalent to [`bind`].
/// Inside `scope!`, `.with(local)` generates the projection.
pub trait WithParams: Sized {
    fn with<Binding>(self, binding: Binding) -> Bound<Self, Binding> {
        bind(self, binding)
    }
}

impl<Node> WithParams for Node {}

impl<Context, Act, Locals: 'static, Node, Binding, NodeState, NodeMemory>
    BtNode<Context, Act, &mut Locals> for Bound<Node, Binding>
where
    Binding: ParamBinding<Locals>,
    Node: for<'a> BtNode<Context, Act, Binding::Params<'a>, State = NodeState, Memory = NodeMemory>,
    NodeState: Default + Send + 'static,
    NodeMemory: Default + Send + 'static,
{
    type State = NodeState;
    type Memory = NodeMemory;
    const NODES: usize = <Node as BtNode<Context, Act, Binding::Params<'static>>>::NODES;

    fn update(
        &self,
        state: &mut NodeState,
        memory: &mut NodeMemory,
        ctx: &mut Context,
        locals: &mut Locals,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        let Some(params) = self.binding.get(locals) else {
            return entry.error(format_args!(
                "bound input is unavailable for {}",
                core::any::type_name::<Node>()
            ));
        };
        self.node.update(state, memory, ctx, params, entry)
    }

    fn inspect(
        &self,
        state: Option<&NodeState>,
        memory: &NodeMemory,
        inspector: &mut dyn Inspector,
    ) {
        BtNode::<Context, Act, Binding::Params<'_>>::inspect(&self.node, state, memory, inspector);
    }
}

pub struct WithoutParams<Node>(Node);

/// Adapts a unit-parameter node to any parameter contract.
pub fn no_params<Node>(node: Node) -> WithoutParams<Node> {
    WithoutParams(node)
}

impl<Context, Act, Params, Node: BtNode<Context, Act>> BtNode<Context, Act, Params>
    for WithoutParams<Node>
{
    type State = Node::State;
    type Memory = Node::Memory;
    const NODES: usize = Node::NODES;

    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut Context,
        _: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        self.0.update(state, memory, ctx, (), entry)
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &Self::Memory,
        inspector: &mut dyn Inspector,
    ) {
        self.0.inspect(state, memory, inspector);
    }
}
