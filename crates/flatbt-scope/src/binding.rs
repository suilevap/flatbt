use std::marker::PhantomData;

use flatbt_core::params::ParamShape;
use flatbt_core::{BtNode, EntryMode, NodeResult};

/// Projects local fields into borrowed node parameters.
pub trait ParamBinding<L> {
    type Params<'a>
    where
        L: 'a;

    fn get<'a>(&self, locals: &'a mut L) -> Option<Self::Params<'a>>;
}

pub struct ReadBinding<F>(F);

/// Binds a shared input. Return None for a missing value.
pub fn read<L, T, F>(project: F) -> ReadBinding<F>
where
    F: Fn(&L) -> Option<&T>,
{
    ReadBinding(project)
}

impl<L, T: 'static, F> ParamBinding<L> for ReadBinding<F>
where
    F: Fn(&L) -> Option<&T>,
{
    type Params<'a>
        = &'a T
    where
        L: 'a;

    fn get<'a>(&self, locals: &'a mut L) -> Option<&'a T> {
        (self.0)(locals)
    }
}

pub struct WriteBinding<F>(F);

/// Binds an exclusive parameter, usually an `Option<T>` output slot.
pub fn write<L, T, F>(project: F) -> WriteBinding<F>
where
    F: Fn(&mut L) -> &mut T,
{
    WriteBinding(project)
}

impl<L, T: 'static, F> ParamBinding<L> for WriteBinding<F>
where
    F: Fn(&mut L) -> &mut T,
{
    type Params<'a>
        = &'a mut T
    where
        L: 'a;

    fn get<'a>(&self, locals: &'a mut L) -> Option<&'a mut T> {
        Some((self.0)(locals))
    }
}

pub struct ParamsBinding<P, F> {
    project: F,
    shape: PhantomData<fn() -> P>,
}

/// Binds multiple parameters with one projection; Rust checks disjoint writes.
/// Shape example: `(Read<Enemy>, Write<Option<Position>>)`. Custom [`ParamShape`]
/// implementations can use named fields.
pub fn params<P, L, F>(project: F) -> ParamsBinding<P, F>
where
    P: ParamShape,
    F: for<'a> Fn(&'a mut L) -> Option<P::Value<'a>>,
{
    ParamsBinding {
        project,
        shape: PhantomData,
    }
}

impl<L, P, F> ParamBinding<L> for ParamsBinding<P, F>
where
    P: ParamShape,
    F: for<'a> Fn(&'a mut L) -> Option<P::Value<'a>>,
{
    type Params<'a>
        = P::Value<'a>
    where
        L: 'a;

    fn get<'a>(&self, locals: &'a mut L) -> Option<Self::Params<'a>> {
        (self.project)(locals)
    }
}

pub struct Bound<N, B> {
    node: N,
    binding: B,
}

/// Binds local fields to node parameters on each update; context and state stay
/// unchanged. Missing inputs log a diagnostic and fail without calling the node.
/// Output writes survive Failure. State cannot retain parameter borrows.
pub fn bind<N, B>(node: N, binding: B) -> Bound<N, B> {
    Bound { node, binding }
}

/// Adds `node.with(binding)`, equivalent to [`bind`].
/// Inside `scope!`, `.with(local)` generates the projection.
pub trait WithParams: Sized {
    fn with<B>(self, binding: B) -> Bound<Self, B> {
        bind(self, binding)
    }
}

impl<N> WithParams for N {}

impl<C, L: 'static, N, B, S> BtNode<C, &mut L> for Bound<N, B>
where
    B: ParamBinding<L>,
    N: for<'a> BtNode<C, B::Params<'a>, State = S>,
    S: Default + Send + 'static,
{
    type State = S;

    fn update(&self, state: &mut S, ctx: &mut C, locals: &mut L, mode: EntryMode) -> NodeResult {
        let Some(params) = self.binding.get(locals) else {
            return NodeResult::error(format_args!(
                "bound input is unavailable for {}",
                std::any::type_name::<N>()
            ));
        };
        self.node.update(state, ctx, params, mode)
    }
}

pub struct WithoutParams<N>(N);

/// Adapts a unit-parameter node to any parameter contract.
pub fn no_params<N>(node: N) -> WithoutParams<N> {
    WithoutParams(node)
}

impl<C, P, N: BtNode<C>> BtNode<C, P> for WithoutParams<N> {
    type State = N::State;

    fn update(&self, state: &mut Self::State, ctx: &mut C, _: P, mode: EntryMode) -> NodeResult {
        self.0.update(state, ctx, (), mode)
    }
}
