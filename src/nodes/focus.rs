use core::marker::PhantomData;

use crate::{BtNode, EntryMode, NodeResult};

/// A subtree run over part of the context.
pub struct Focus<F, N, D> {
    lens: F,
    child: N,
    part: PhantomData<fn() -> D>,
}

/// Runs `child`, a tree over `D`, on the part of the context `lens` selects,
/// so one subtree serves several blackboards that contain a `D`.
///
/// ```
/// use flatbt::prelude::*;
///
/// struct Legs { steps: u32 }
/// struct Agent { legs: Legs }
///
/// fn walk() -> impl BtNode<Legs> {
///     leaf(|legs: &mut Legs| { legs.steps += 1; NodeResult::Success })
/// }
///
/// let tree = focus(|agent: &mut Agent| &mut agent.legs, walk());
/// let mut state: BtState<_, _> = BtState::new(&tree);
/// let mut agent = Agent { legs: Legs { steps: 0 } };
/// assert_eq!(update(&tree, &mut state, &mut agent, EntryMode::Evaluate), NodeResult::Success);
/// assert_eq!(agent.legs.steps, 1);
/// ```
pub fn focus<C, D, F, N>(lens: F, child: N) -> Focus<F, N, D>
where
    F: Fn(&mut C) -> &mut D,
{
    Focus {
        lens,
        child,
        part: PhantomData,
    }
}

impl<C, D, A, P, F, N> BtNode<C, A, P> for Focus<F, N, D>
where
    F: Fn(&mut C) -> &mut D,
    N: BtNode<D, A, P>,
{
    type State = N::State;

    #[inline]
    fn update(
        &self,
        state: &mut N::State,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult<A> {
        self.child.update(state, (self.lens)(ctx), params, mode)
    }
}
