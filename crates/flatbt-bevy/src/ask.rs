use bevy_ecs::prelude::*;
use flatbt_nodes::{ActionNode, BtAction, action};

use crate::{BehaviorContext, Blackboard};

/// A question put to the world, waiting for a system to answer it.
///
/// Built by [`ask`].
pub struct Ask<B, P> {
    request: B,
    answered: P,
}

/// Inserts `request` on the agent, then runs until `answered` holds.
///
/// A [`BehaviorContext`] declares its access up front, so a tree cannot run an
/// arbitrary query -- it can only ask. Inserting a marker component is the ask;
/// an ordinary system matching that marker is the answer, written wherever the
/// game already keeps that knowledge; and `answered` is how the tree notices,
/// reading through its own agent view.
///
/// It is an action rather than a leaf because a leaf returning
/// [`NodeResult::Running`](flatbt_core::NodeResult::Running) is re-entered on
/// every resume, so a leaf that asks would ask again every tick and move the
/// entity between archetypes twice a frame. `start` runs once per invocation,
/// which is what asking means. Nothing is inserted when `answered` already
/// holds, so a standing answer costs one predicate call.
///
/// Fails if the request is never answered and the branch is abandoned; succeeds
/// the moment it is, so the node after it can use the answer.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Component, Clone)]
/// struct WantsCover;
///
/// #[derive(Component)]
/// struct CoverTarget(Vec2);
/// # #[derive(Clone, Copy)]
/// # struct Vec2;
///
/// #[derive(QueryData)]
/// #[query_data(mutable)]
/// struct Fighter {
///     cover: Option<&'static CoverTarget>,
/// }
/// # impl BehaviorContext for Fighter { type Agent = Self; type Param = (); }
///
/// fn hide() -> impl BehaviorNode<Fighter> {
///     seq((
///         ask(WantsCover, |bb: &Blackboard<Fighter>| bb.cover.is_some()),
///         leaf(|_bb: &mut Blackboard<Fighter>| NodeResult::Success),
///     ))
/// }
/// ```
pub fn ask<B, P>(request: B, answered: P) -> ActionNode<Ask<B, P>> {
    action(Ask { request, answered })
}

impl<C, B, P> BtAction<Blackboard<'_, '_, '_, '_, '_, C>> for Ask<B, P>
where
    C: BehaviorContext,
    B: Bundle + Clone,
    P: Fn(&Blackboard<'_, '_, '_, '_, '_, C>) -> bool,
{
    type State = ();

    fn start(&self, bb: &mut Blackboard<'_, '_, '_, '_, '_, C>, _: ()) -> Option<()> {
        if !(self.answered)(bb) {
            let request = self.request.clone();
            bb.agent_commands().insert(request);
        }
        Some(())
    }

    fn is_in_progress(&self, _: &(), bb: &Blackboard<'_, '_, '_, '_, '_, C>, _: ()) -> bool {
        !(self.answered)(bb)
    }
}
