use std::marker::PhantomData;

use crate::params::ParamShape;
use crate::{BtNode, EntryMode, NodeResult};

/// Provides a node's parameters by borrowing explicitly selected local fields.
pub trait ParamBinding<L> {
    type Params<'a>
    where
        L: 'a;

    fn get<'a>(&self, locals: &'a mut L) -> Option<Self::Params<'a>>;
}

pub struct ReadBinding<F>(F);

/// Binds one shared input. Return None when its producer has not supplied it.
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

/// Binds one exclusive parameter. Output nodes usually request `&mut Option<T>`.
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

/// Binds several inputs/outputs in one projection, so Rust checks disjoint writes.
/// For example use `(Read<Enemy>, Write<Option<Position>>)` as the shape.
/// A custom ParamShape can describe named parameter fields instead of a tuple.
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

/// Adapts the scope's local fields to the node's declared parameter contract.
/// The context type and node state are unchanged. Parameters are borrowed anew
/// on every update and cannot be retained in the node's static state.
/// An unavailable input reports a diagnostic and fails without calling the node.
/// Effects on local output slots, like BB effects, are not rolled back on Failure.
pub fn bind<N, B>(node: N, binding: B) -> Bound<N, B> {
    Bound { node, binding }
}

/// Fluent spelling of bind for ordinary Rust tree construction.
/// In scope!, `.with(local)` generates the field binding automatically.
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

/// Reuses a node with no parameters inside any scope.
pub fn no_params<N>(node: N) -> WithoutParams<N> {
    WithoutParams(node)
}

impl<C, P, N: BtNode<C>> BtNode<C, P> for WithoutParams<N> {
    type State = N::State;

    fn update(&self, state: &mut Self::State, ctx: &mut C, _: P, mode: EntryMode) -> NodeResult {
        self.0.update(state, ctx, (), mode)
    }
}
