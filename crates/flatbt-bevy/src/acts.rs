use core::marker::PhantomData;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};

use crate::BehaviorSystems;

/// All action-component syncs. Order game systems against this, or against
/// [`BehaviorSystems`] when the distinction does not matter.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionSystems;

/// Makes one of a tree's decisions an ordinary component.
///
/// A node sees `&mut C` and nothing else, so what a tree decides starts life as
/// a field. That is fine between nodes and poor as an interface to the rest of
/// the game: an ECS matches on components, not on somebody's struct field. This
/// carries the field across, one plugin per component:
///
/// ```
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Component, Default, PartialEq)]
/// struct Guard {
///     reloading: bool,
///     march_to: Option<f32>,
/// }
///
/// /// The agent is reloading. A marker, so systems can filter on it.
/// #[derive(Component, Default, PartialEq)]
/// struct Reloading;
///
/// /// The agent is walking somewhere, and this is where.
/// #[derive(Component, PartialEq)]
/// struct MarchingTo(f32);
///
/// # fn patrol() -> impl BehaviorNode<Guard> { check(|_: &Guard| true) }
/// # let mut app = App::new();
/// app.add_plugins(BehaviorPlugin::for_tree(patrol))
///     // A marker names itself, since the predicate does not mention it.
///     .add_plugins(ActionComponent::<_, Reloading>::while_(|g: &Guard| g.reloading))
///     .add_plugins(ActionComponent::describing(|g: &Guard| {
///         g.march_to.map(MarchingTo)
///     }));
/// ```
///
/// After that `Query<&mut Ammo, With<Reloading>>` and
/// `Query<(&mut Transform, &MarchingTo)>` are how the game does the work, and
/// nothing outside the tree's own module reads `Guard`.
///
/// # What it costs
///
/// Adding or removing a component moves the entity between archetypes, so the
/// price is set by how often a decision *changes*, not by how many agents there
/// are. Over 100 000 agents, whole frame, against the same decision left as a
/// field:
///
/// | an action lasts | as a field | as a component |
/// | --- | --- | --- |
/// | 1 tick | 0.57 ms | 8.06 ms |
/// | 10 ticks | 0.54 ms | 2.05 ms |
/// | 30 ticks | 0.51 ms | 1.28 ms |
/// | 120 ticks | 0.49 ms | 0.71 ms |
///
/// So this belongs on decisions that stand: an action that runs to completion
/// over many ticks, a destination an agent walks to. A decision retaken every
/// tick should stay a field, and usually means the action wants writing as a
/// `BtAction` that spans ticks rather than a leaf that
/// re-decides.
///
/// The sync is one pass per registered component, after the tick, and it queues
/// nothing where the answer and the component already agree. Inserts are
/// batched; removals are not, since Bevy has no batched remove. In
/// `games/arena`, where five decisions are registered over 100 000 fighters
/// that change their minds constantly, that comes to about 0.5-0.8 ms of frame
/// per registered component -- the archetype moves themselves, not the
/// bookkeeping, which batching them barely touched. At a thousand agents it is
/// tens of microseconds.
///
/// The alternative is to leave the decision a field and have the game read the
/// blackboard. That is cheaper and worse: it makes the blackboard an interface,
/// and every system that reads it has to know which fields are the tree's
/// output and which are its input.
pub struct ActionComponent<C, M> {
    read: Read<C, M>,
    schedule: InternedScheduleLabel,
    blackboard: PhantomData<fn() -> C>,
}

/// How the component's presence and contents are read off the blackboard.
enum Read<C, M> {
    /// Present while the predicate holds, carrying what the constructor makes.
    While(fn(&C) -> bool, fn() -> M),
    /// Present while this returns a value, carrying it.
    Describing(fn(&C) -> Option<M>),
}

impl<C, M> ActionComponent<C, M>
where
    C: Component,
    M: Component<Mutability = Mutable> + PartialEq,
{
    /// The component is present exactly while `read` returns a value, and
    /// carries that value.
    pub fn describing(read: fn(&C) -> Option<M>) -> Self {
        Self::new(Read::Describing(read))
    }

    /// Syncs in `schedule` instead of [`Update`]. Use the one the tree ticks in.
    pub fn in_schedule(mut self, schedule: impl ScheduleLabel) -> Self {
        self.schedule = schedule.intern();
        self
    }

    fn new(read: Read<C, M>) -> Self {
        Self {
            read,
            schedule: Update.intern(),
            blackboard: PhantomData,
        }
    }
}

impl<C, M> ActionComponent<C, M>
where
    C: Component,
    M: Component<Mutability = Mutable> + PartialEq + Default,
{
    /// The component is present exactly while `active` holds. For a marker with
    /// no data of its own.
    pub fn while_(active: fn(&C) -> bool) -> Self {
        // `M::default` as a function pointer, so the plugin itself needs no
        // `Default` bound and a component carrying data stays usable.
        Self::new(Read::While(active, M::default))
    }
}

impl<C, M> Plugin for ActionComponent<C, M>
where
    C: Component,
    M: Component<Mutability = Mutable> + PartialEq,
{
    fn build(&self, app: &mut App) {
        let read = self.read;
        app.add_systems(
            self.schedule,
            (move |mut agents: Query<(Entity, &C, Option<&mut M>)>,
                   mut arriving: Local<Vec<(Entity, M)>>,
                   mut leaving: Local<Vec<Entity>>,
                   mut commands: Commands| {
                for (entity, blackboard, current) in agents.iter_mut() {
                    let wanted = match read {
                        Read::While(active, make) => active(blackboard).then(make),
                        Read::Describing(read) => read(blackboard),
                    };
                    match (wanted, current) {
                        // The common case by far: the decision stands and the
                        // component is already there, so nothing is queued.
                        (Some(wanted), Some(mut held)) => {
                            held.set_if_neq(wanted);
                        }
                        (Some(wanted), None) => arriving.push((entity, wanted)),
                        (None, Some(_)) => leaving.push(entity),
                        (None, None) => {}
                    }
                }
                // Batched: one archetype move per entity either way, but a
                // single command for the whole population rather than one each.
                if !arriving.is_empty() {
                    commands.try_insert_batch(core::mem::take(&mut *arriving));
                }
                for entity in leaving.drain(..) {
                    commands.entity(entity).try_remove::<M>();
                }
            })
            .in_set(ActionSystems)
            .after(BehaviorSystems),
        );
    }
}

impl<C, M> Clone for Read<C, M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C, M> Copy for Read<C, M> {}
