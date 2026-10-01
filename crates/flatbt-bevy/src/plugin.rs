use bevy_app::{App, Plugin, Update};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};
use bevy_utils::Parallel;

use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use bevy_time::Time;

use crate::debug::describe_behaviors;
use crate::{Behavior, BehaviorTree, Tick, TickAt, TickFn, TreeBuilder};

/// All behavior ticks, whatever their tree. Order other systems against this.
///
/// An act component is up to date after this set: a system reading `&Act` runs
/// after it, and a system filling the blackboard runs before it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BehaviorSystems;

/// Builds one tree and registers the system that ticks its agents.
///
/// One per tree. The tree is built when the app is built, so the tick system is
/// in place before any agent exists and any schedule will do. Adding the same
/// builder twice is rejected by Bevy as a duplicate plugin.
///
/// ```no_run
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Component, Default)]
/// # struct Guard { ammo: u32 }
/// #[derive(Component, Clone, Copy, PartialEq)]
/// enum Act {
///     Firing,
/// }
///
/// fn shoot() -> impl BehaviorNode<Guard, Act> {
///     leaf(|guard: &mut Guard| {
///         if guard.ammo > 0 {
///             NodeResult::Running(Act::Firing)
///         } else {
///             NodeResult::Failure
///         }
///     })
/// }
///
/// App::new().add_plugins(BehaviorPlugin::for_tree(shoot));
/// ```
pub struct BehaviorPlugin<
    Blackboard: Send + Sync + 'static,
    Act: Send + Sync + 'static,
    Builder: TreeBuilder<Blackboard, Act>,
> {
    builder: Builder,
    tick_mode: TickFn<Blackboard>,
    schedule: InternedScheduleLabel,
    parallel: bool,
    act: core::marker::PhantomData<fn() -> Act>,
}

impl<Blackboard, Act, Builder> BehaviorPlugin<Blackboard, Act, Builder>
where
    Blackboard: Component<Mutability = Mutable>,
    Act: Component<Mutability = Mutable> + PartialEq,
    Builder: TreeBuilder<Blackboard, Act>,
{
    /// Builds the tree once and ticks its agents in [`Update`], one at a time,
    /// in query order. All three type parameters come from the builder.
    pub fn for_tree(builder: Builder) -> Self {
        Self {
            builder,
            tick_mode: |_, _| Tick::Evaluate,
            schedule: Update.intern(),
            parallel: false,
            act: core::marker::PhantomData,
        }
    }

    /// Decides, per agent per tick, whether the tree is entered at all and how.
    ///
    /// Defaults to [`Tick::Evaluate`], which is the answer that is always
    /// correct: the other two are optimisations that cost an agent
    /// responsiveness and never change what it does once it runs. Reach for
    /// them when a profile says to, and expect the tree to behave the same
    /// under all three -- see [`Tick`].
    ///
    /// The answer comes from the blackboard and a [`TickAt`]: the agent and the
    /// schedule's clock. Anything else it needs -- a perception flag, whose turn
    /// it is -- is gathered like everything else. See
    /// [`evaluate_every`](crate::evaluate_every) and
    /// [`act_every`](crate::act_every).
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// # use flatbt_bevy::prelude::*;
    /// # #[derive(Component, Default)]
    /// # struct Guard { alarm_changed: bool, walking: bool }
    /// # #[derive(Component, Clone, Copy, PartialEq)]
    /// # enum Act { Idle }
    /// # fn patrol() -> impl BehaviorNode<Guard, Act> {
    /// #     leaf(|_: &mut Guard| NodeResult::Running(Act::Idle))
    /// # }
    /// BehaviorPlugin::for_tree(patrol).tick_mode(|guard: &Guard, _| {
    ///     if guard.alarm_changed {
    ///         Tick::Evaluate
    ///     } else if guard.walking {
    ///         // Systems are carrying out the standing act; nothing to add.
    ///         Tick::Skip
    ///     } else {
    ///         Tick::Resume
    ///     }
    /// });
    /// ```
    pub fn tick_mode(mut self, tick_mode: TickFn<Blackboard>) -> Self {
        self.tick_mode = tick_mode;
        self
    }

    /// Ticks in `schedule` instead of [`Update`]. Use [`FixedUpdate`] for
    /// simulation-rate trees.
    ///
    /// [`FixedUpdate`]: bevy_app::FixedUpdate
    pub fn in_schedule(mut self, schedule: impl ScheduleLabel) -> Self {
        self.schedule = schedule.intern();
        self
    }

    /// Spreads agents across the task pool with [`Query::par_iter_mut`].
    ///
    /// A tree touches its own blackboard and its own act, both disjoint per
    /// entity, so this needs no further declaration. Acts that appear or
    /// disappear are queued per thread and applied right after the tick, as in
    /// the serial one. Iteration order becomes unspecified, which nothing in a
    /// tree can observe.
    pub fn parallel(mut self) -> Self {
        self.parallel = true;
        self
    }
}

impl<Blackboard, Act, Builder> Plugin for BehaviorPlugin<Blackboard, Act, Builder>
where
    Blackboard: Component<Mutability = Mutable>,
    Act: Component<Mutability = Mutable> + PartialEq,
    Builder: TreeBuilder<Blackboard, Act>,
{
    fn build(&self, app: &mut App) {
        app.insert_resource(BehaviorTree::<Blackboard, Act, Builder>::new(
            &self.builder,
            self.tick_mode,
        ))
        .insert_resource(ActChanges::<Blackboard, Act, Builder>::default());
        let apply = apply_act_changes::<Blackboard, Act, Builder>
            .run_if(has_act_changes::<Blackboard, Act, Builder>);
        if self.parallel {
            app.add_systems(
                self.schedule,
                (
                    tick_behaviors_parallel::<Blackboard, Act, Builder>,
                    apply,
                    describe_behaviors::<Blackboard, Act, Builder>,
                )
                    .chain()
                    .in_set(BehaviorSystems),
            );
        } else {
            app.add_systems(
                self.schedule,
                (
                    tick_behaviors::<Blackboard, Act, Builder>,
                    apply,
                    describe_behaviors::<Blackboard, Act, Builder>,
                )
                    .chain()
                    .in_set(BehaviorSystems),
            );
        }
    }
}

/// The agents one tick system drives: their saved state, their blackboard, and
/// whatever they are currently doing.
///
/// The act is `Option<&mut Act>` because an agent that decided nothing carries no
/// act component at all -- "doing nothing" is the absence of the component, not
/// a variant of it. An act that merely *changes* is written in place, so an
/// agent that keeps doing the same kind of thing never moves archetype.
type Agents<'w, 's, Blackboard, Act, Builder> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Behavior<Blackboard, Act, Builder>,
        &'static mut Blackboard,
        Option<&'static mut Act>,
    ),
>;

/// Ticks every agent running the tree named by `Builder`, in query order.
fn tick_behaviors<Blackboard, Act, Builder>(
    tree: Res<BehaviorTree<Blackboard, Act, Builder>>,
    time: Option<Res<Time>>,
    mut agents: Agents<Blackboard, Act, Builder>,
    changes: Res<ActChanges<Blackboard, Act, Builder>>,
) where
    Blackboard: Component<Mutability = Mutable>,
    Act: Component<Mutability = Mutable> + PartialEq,
    Builder: TreeBuilder<Blackboard, Act>,
{
    let clock = clock(time.as_deref());
    for (entity, behavior, bb, held) in agents.iter_mut() {
        changes.record(entity, tick_agent(&tree, clock(entity), behavior, bb, held));
    }
}

/// Ticks every agent running the tree named by `Builder` across the task pool.
fn tick_behaviors_parallel<Blackboard, Act, Builder>(
    tree: Res<BehaviorTree<Blackboard, Act, Builder>>,
    time: Option<Res<Time>>,
    mut agents: Agents<Blackboard, Act, Builder>,
    changes: Res<ActChanges<Blackboard, Act, Builder>>,
) where
    Blackboard: Component<Mutability = Mutable>,
    Act: Component<Mutability = Mutable> + PartialEq,
    Builder: TreeBuilder<Blackboard, Act>,
{
    let clock = clock(time.as_deref());
    agents
        .par_iter_mut()
        .for_each(|(entity, behavior, bb, held)| {
            changes.record(entity, tick_agent(&tree, clock(entity), behavior, bb, held));
        });
}

/// Where each agent's tick falls this run: zero without a `Time` resource.
fn clock(time: Option<&Time>) -> impl Fn(Entity) -> TickAt + Sync {
    let (elapsed, delta) = time.map_or((Duration::ZERO, Duration::ZERO), |time| {
        (time.elapsed(), time.delta())
    });
    move |entity| TickAt {
        entity,
        elapsed,
        delta,
    }
}

/// One agent's tick, and what it left for a command.
fn tick_agent<Blackboard, Act, Builder>(
    tree: &BehaviorTree<Blackboard, Act, Builder>,
    at: TickAt,
    mut behavior: Mut<'_, Behavior<Blackboard, Act, Builder>>,
    mut bb: Mut<'_, Blackboard>,
    held: Option<Mut<'_, Act>>,
) -> ActChange<Act>
where
    Blackboard: Component<Mutability = Mutable>,
    Act: Component<Mutability = Mutable> + PartialEq,
    Builder: TreeBuilder<Blackboard, Act>,
{
    // Whether a blackboard changed is the gather's business, not the tick's: it
    // is rewritten every tick anyway, and marking the whole population changed
    // would drag the rest of the engine along. Nodes may write to it -- it is
    // how they leave notes for each other -- and those writes are *not* visible
    // to `Changed<Blackboard>` either. The blackboard is the tree's input; its output is
    // the act.
    let bb = bb.bypass_change_detection();
    let Some(mode) = tree.tick_mode()(bb, at).entry_mode() else {
        // Skipped: the suspended invocation and the standing act stay as they
        // are, and the systems carrying that act out keep seeing it.
        return ActChange::Settled;
    };
    match (behavior.tick(tree.get(), bb, mode), held) {
        // The common case by far: still doing something, so the component is
        // already there and only its contents can change.
        (Some(act), Some(mut held)) => {
            held.set_if_neq(act);
            ActChange::Settled
        }
        (Some(act), None) => ActChange::Appeared(act),
        (None, Some(_)) => ActChange::Gone,
        (None, None) => ActChange::Settled,
    }
}

/// What a tick left for a command: an act appearing or going is a structural
/// change, which a tick cannot make itself.
enum ActChange<Act> {
    /// Written in place, or nothing to write.
    Settled,
    Appeared(Act),
    Gone,
}

/// Acts that appeared or went during a tick, waiting for `apply_act_changes`.
///
/// Why not `Commands`: a system with deferred parameters makes Bevy's
/// multi-threaded executor run `ApplyDeferred` every frame, which spawns a task
/// and allocates whether or not anything was queued. Queues here are per
/// thread, keep their capacity, and are applied by an exclusive system whose
/// run condition skips it -- at no cost -- on the common frame where every act
/// was only written in place.
#[derive(Resource)]
struct ActChanges<Blackboard, Act: Send, Builder> {
    queued: Parallel<Vec<(Entity, Option<Act>)>>,
    any: AtomicBool,
    names: PhantomData<fn() -> (Blackboard, Builder)>,
}

impl<Blackboard, Act: Send, Builder> Default for ActChanges<Blackboard, Act, Builder> {
    fn default() -> Self {
        Self {
            queued: Parallel::default(),
            any: AtomicBool::new(false),
            names: PhantomData,
        }
    }
}

impl<Blackboard, Act: Send, Builder> ActChanges<Blackboard, Act, Builder> {
    fn record(&self, entity: Entity, change: ActChange<Act>) {
        let act = match change {
            ActChange::Settled => return,
            ActChange::Appeared(act) => Some(act),
            ActChange::Gone => None,
        };
        self.queued.scope(|queue| queue.push((entity, act)));
        self.any.store(true, Ordering::Relaxed);
    }
}

fn has_act_changes<Blackboard, Act, Builder>(
    changes: Res<ActChanges<Blackboard, Act, Builder>>,
) -> bool
where
    Blackboard: Send + Sync + 'static,
    Act: Send + Sync + 'static,
    Builder: TreeBuilder<Blackboard, Act>,
{
    changes.any.load(Ordering::Relaxed)
}

/// Inserts acts that appeared and removes those that went.
///
/// Chained right after the tick inside [`BehaviorSystems`], so an act is up to
/// date when the set ends, with no sync point needed.
fn apply_act_changes<Blackboard, Act, Builder>(world: &mut World)
where
    Blackboard: Send + Sync + 'static,
    Act: Component,
    Builder: TreeBuilder<Blackboard, Act>,
{
    world.resource_scope(
        |world, mut changes: Mut<ActChanges<Blackboard, Act, Builder>>| {
            *changes.any.get_mut() = false;
            for queue in changes.queued.iter_mut() {
                // `drain(..)` rather than `Parallel::drain`, which takes the vector
                // and its capacity with it.
                for (entity, act) in queue.drain(..) {
                    // Gone when a system despawned it after the tick.
                    let Ok(mut agent) = world.get_entity_mut(entity) else {
                        continue;
                    };
                    match act {
                        Some(act) => {
                            agent.insert(act);
                        }
                        None => {
                            agent.remove::<Act>();
                        }
                    }
                }
            }
        },
    );
}
