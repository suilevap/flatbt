use bevy_ecs::prelude::*;
use flatbt_nodes::{ActionNode, BtAction};

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
pub fn ask<B, P>(request: B, answered: P) -> ActionNode<Act<Ask<B, P>>> {
    act(Ask { request, answered })
}

impl<C, B, P> AgentAction<C> for Ask<B, P>
where
    C: BehaviorContext,
    B: Bundle + Clone,
    P: Fn(&Blackboard<C>) -> bool,
{
    type State = ();

    fn start(&self, bb: &mut Blackboard<C>) -> Option<()> {
        if !(self.answered)(bb) {
            let request = self.request.clone();
            bb.agent_commands().insert(request);
        }
        Some(())
    }

    fn is_in_progress(&self, _: &(), bb: &Blackboard<C>) -> bool {
        !(self.answered)(bb)
    }
}

/// An action over a [`Blackboard`], written without naming its lifetimes.
///
/// [`BtAction`] is generic over its context, so implementing it for a
/// blackboard means writing `Blackboard<'_, '_, '_, '_, '_, C>` in the impl
/// header and again in every method. Those five lifetimes are an artefact of
/// assembling the blackboard from borrows, and nothing an action ever names.
/// This trait is the same lifecycle with the context fixed, so a method
/// signature can elide them the way an ordinary function does -- and it drops
/// the parameter argument, which a Bevy tree does not use.
///
/// Implement it, and [`act`] turns it into a node.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Component)]
/// struct Ammo(u32);
/// # #[derive(QueryData)]
/// # #[query_data(mutable)]
/// # struct Guard { ammo: &'static mut Ammo }
/// # impl BehaviorContext for Guard { type Agent = Self; type Param = (); }
///
/// /// Takes `ticks` updates, then refills.
/// struct Reload {
///     ticks: u32,
///     rounds: u32,
/// }
///
/// impl AgentAction<Guard> for Reload {
///     type State = u32;
///
///     fn start(&self, _: &mut Blackboard<Guard>) -> Option<u32> {
///         Some(0)
///     }
///
///     fn is_in_progress(&self, elapsed: &u32, _: &Blackboard<Guard>) -> bool {
///         *elapsed < self.ticks
///     }
///
///     fn tick(&self, elapsed: &mut u32, _: &mut Blackboard<Guard>) {
///         *elapsed += 1;
///     }
///
///     fn complete(&self, _: &mut u32, bb: &mut Blackboard<Guard>) -> bool {
///         bb.ammo.0 = self.rounds;
///         true
///     }
/// }
///
/// fn refill() -> impl BehaviorNode<Guard> {
///     act(Reload { ticks: 2, rounds: 6 })
/// }
/// ```
pub trait AgentAction<C: BehaviorContext> {
    /// What the action keeps between updates. Dropped when it ends.
    type State: Send + 'static;

    /// Runs once when the action is entered. `None` fails the action.
    fn start(&self, bb: &mut Blackboard<C>) -> Option<Self::State>;

    /// Whether the action is still going. False ends it through `complete`.
    fn is_in_progress(&self, state: &Self::State, bb: &Blackboard<C>) -> bool;

    /// Runs inline while the action is in progress. Defaults to no work.
    fn tick(&self, _state: &mut Self::State, _bb: &mut Blackboard<C>) {}

    /// Runs once when the action ends, before its state drops. Returns whether
    /// it succeeded.
    fn complete(&self, _state: &mut Self::State, _bb: &mut Blackboard<C>) -> bool {
        true
    }
}

/// An [`AgentAction`] as [`BtAction`] takes it. Built by [`act`].
///
/// A wrapper rather than a blanket impl on the action itself: `BtAction` comes
/// from another crate, so the impl needs a type of this one to hang on.
pub struct Act<A>(A);

/// Runs an [`AgentAction`] as a node.
///
/// The counterpart of [`flatbt_nodes::action`], which takes a [`BtAction`] and
/// works here too -- at the cost of spelling the blackboard's lifetimes out.
pub fn act<A>(action: A) -> ActionNode<Act<A>> {
    flatbt_nodes::action(Act(action))
}

impl<C: BehaviorContext, A: AgentAction<C>> BtAction<Blackboard<'_, '_, '_, '_, '_, C>> for Act<A> {
    type State = A::State;

    fn start(&self, bb: &mut Blackboard<'_, '_, '_, '_, '_, C>, _: ()) -> Option<A::State> {
        self.0.start(bb)
    }

    fn is_in_progress(
        &self,
        state: &A::State,
        bb: &Blackboard<'_, '_, '_, '_, '_, C>,
        _: (),
    ) -> bool {
        self.0.is_in_progress(state, bb)
    }

    fn tick(&self, state: &mut A::State, bb: &mut Blackboard<'_, '_, '_, '_, '_, C>, _: ()) {
        self.0.tick(state, bb);
    }

    fn complete(
        &self,
        state: &mut A::State,
        bb: &mut Blackboard<'_, '_, '_, '_, '_, C>,
        _: (),
    ) -> bool {
        self.0.complete(state, bb)
    }
}
