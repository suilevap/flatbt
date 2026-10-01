use core::marker::PhantomData;

use crate::inspect::{Inspector, NodeInfo};
use crate::{BtNode, Entry, NodeResult};

/// A subtree run over part of the context.
pub struct Focus<Lens, Child, Focused> {
    lens: Lens,
    child: Child,
    part: PhantomData<fn() -> Focused>,
}

/// Runs `child`, a tree over `Focused`, on the part of the context `lens` selects,
/// so one subtree serves several blackboards that contain a `Focused`.
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
pub fn focus<Context, Focused, Lens, Child>(lens: Lens, child: Child) -> Focus<Lens, Child, Focused>
where
    Lens: Fn(&mut Context) -> &mut Focused,
{
    Focus {
        lens,
        child,
        part: PhantomData,
    }
}

impl<Context, Focused, Act, Params, Lens, Child> BtNode<Context, Act, Params>
    for Focus<Lens, Child, Focused>
where
    Lens: Fn(&mut Context) -> &mut Focused,
    Child: BtNode<Focused, Act, Params>,
{
    type State = Child::State;
    type Memory = Child::Memory;
    const NODES: usize = 1 + Child::NODES;

    #[inline]
    fn update(
        &self,
        state: &mut Child::State,
        memory: &mut Child::Memory,
        ctx: &mut Context,
        params: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        entry.run(1, &self.child, state, memory, (self.lens)(ctx), params)
    }

    fn inspect(
        &self,
        state: Option<&Child::State>,
        memory: &Child::Memory,
        inspector: &mut dyn Inspector,
    ) {
        inspector.node(NodeInfo::new("focus", state.is_some()), |inspector| {
            self.child.inspect(state, memory, inspector);
        });
    }
}
