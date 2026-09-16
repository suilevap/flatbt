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
/// Succeeds the moment the answer is there, so the node after it can use it.
///
/// Bound to an output slot with `.with(out name)` inside [`scope!`], `ask`
/// writes the answer into that local instead of leaving it on the blackboard --
/// which is where the ECS and the scope meet. The question is answered by an
/// ordinary system writing an ordinary component; the answer arrives as an
/// ordinary local, and the nodes after it take a value rather than an `Option`,
/// so they cannot run without one. The predicate returns `Option<T>` in that
/// shape and `bool` in this one.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// # use flatbt_scope::scope;
/// # use flatbt_core::{BtNode, EntryMode, NodeResult};
/// # #[derive(Component, Clone)]
/// # struct WantsCover;
/// # #[derive(Component)]
/// # struct CoverTarget(u32);
/// # struct Fighter { cover: Option<u32> }
/// # #[derive(QueryData)]
/// # struct FighterAccess { cover: Option<&'static CoverTarget> }
/// # impl BehaviorContext for Fighter {
/// #     type Agent = FighterAccess;
/// #     type Param = ();
/// #     type Snapshot = Self;
/// #     fn read(_: Entity, a: &FighterAccessItem, _: &()) -> Fighter { Fighter { cover: a.cover.map(|c| c.0) } }
/// #     fn write(_: &Fighter, _: &mut FighterAccessItem) {}
/// # }
/// # struct WalkTo;
/// # impl BtNode<Blackboard<Fighter>, &u32> for WalkTo {
/// #     type State = ();
/// #     fn update(&self, _: &mut (), _: &mut Blackboard<Fighter>, _: &u32, _: EntryMode) -> NodeResult { NodeResult::Success }
/// # }
/// fn hide() -> impl BehaviorNode<Fighter> {
///     scope! {
///         let spot: u32;
///         sequence {
///             ask(WantsCover, |bb: &Blackboard<Fighter>| bb.cover).with(out spot);
///             WalkTo.with(spot);
///         }
///     }
/// }
/// ```
///
/// The local belongs to the invocation, so leaving the branch and coming back
/// asks again rather than acting on an answer chosen for an older situation.
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
/// struct Fighter {
///     cover: Option<Vec2>,
/// }
///
/// #[derive(QueryData)]
/// struct FighterAccess {
///     cover: Option<&'static CoverTarget>,
/// }
/// # impl BehaviorContext for Fighter {
/// #     type Agent = FighterAccess;
/// #     type Param = ();
/// #     type Snapshot = Self;
/// #     fn read(_: Entity, a: &FighterAccessItem, _: &()) -> Fighter { Fighter { cover: a.cover.map(|c| c.0) } }
/// #     fn write(_: &Fighter, _: &mut FighterAccessItem) {}
/// # }
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

impl<C, B, P> BtAction<Blackboard<C>> for Ask<B, P>
where
    C: BehaviorContext,
    B: Bundle + Clone,
    P: Fn(&Blackboard<C>) -> bool,
{
    type State = ();

    fn start(&self, bb: &mut Blackboard<C>, _: ()) -> Option<()> {
        if !(self.answered)(bb) {
            let request = self.request.clone();
            bb.agent_commands().insert(request);
        }
        Some(())
    }

    fn is_in_progress(&self, _: &(), bb: &Blackboard<C>, _: ()) -> bool {
        !(self.answered)(bb)
    }
}

impl<C, B, P, T> BtAction<Blackboard<C>, &mut Option<T>> for Ask<B, P>
where
    C: BehaviorContext,
    B: Bundle + Clone,
    P: Fn(&Blackboard<C>) -> Option<T>,
    T: 'static,
{
    type State = ();

    fn start(&self, bb: &mut Blackboard<C>, _: &mut Option<T>) -> Option<()> {
        if (self.answered)(bb).is_none() {
            let request = self.request.clone();
            bb.agent_commands().insert(request);
        }
        Some(())
    }

    fn is_in_progress(&self, _: &(), bb: &Blackboard<C>, _: &mut Option<T>) -> bool {
        (self.answered)(bb).is_none()
    }

    /// Fills the slot the moment the answer is there, so the nodes after this
    /// one read a value rather than an `Option`.
    fn complete(&self, _: &mut (), bb: &mut Blackboard<C>, slot: &mut Option<T>) -> bool {
        *slot = (self.answered)(bb);
        true
    }
}
