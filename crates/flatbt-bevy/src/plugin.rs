use bevy_app::{App, Plugin, Update};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};
use bevy_ecs::system::ParallelCommands;

use core::time::Duration;

use bevy_time::Time;

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
pub struct BehaviorPlugin<C: Send + Sync + 'static, A: Send + Sync + 'static, F: TreeBuilder<C, A>>
{
    builder: F,
    tick_mode: TickFn<C>,
    schedule: InternedScheduleLabel,
    parallel: bool,
    act: core::marker::PhantomData<fn() -> A>,
}

impl<C, A, F> BehaviorPlugin<C, A, F>
where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    /// Builds the tree once and ticks its agents in [`Update`], one at a time,
    /// in query order. All three type parameters come from the builder.
    pub fn for_tree(builder: F) -> Self {
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
    pub fn tick_mode(mut self, tick_mode: TickFn<C>) -> Self {
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
    /// entity, so this needs no further declaration. Agents whose act appears
    /// or disappears still go through a command, which is applied when the
    /// schedule syncs. Iteration order becomes unspecified, which nothing in a
    /// tree can observe.
    pub fn parallel(mut self) -> Self {
        self.parallel = true;
        self
    }
}

impl<C, A, F> Plugin for BehaviorPlugin<C, A, F>
where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    fn build(&self, app: &mut App) {
        app.insert_resource(BehaviorTree::<C, A, F>::new(&self.builder, self.tick_mode));
        if self.parallel {
            app.add_systems(
                self.schedule,
                tick_behaviors_parallel::<C, A, F>.in_set(BehaviorSystems),
            );
        } else {
            app.add_systems(
                self.schedule,
                tick_behaviors::<C, A, F>.in_set(BehaviorSystems),
            );
        }
    }
}

/// The agents one tick system drives: their saved state, their blackboard, and
/// whatever they are currently doing.
///
/// The act is `Option<&mut A>` because an agent that decided nothing carries no
/// act component at all -- "doing nothing" is the absence of the component, not
/// a variant of it. An act that merely *changes* is written in place, so an
/// agent that keeps doing the same kind of thing never moves archetype.
type Agents<'w, 's, C, A, F> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Behavior<C, A, F>,
        &'static mut C,
        Option<&'static mut A>,
    ),
>;

/// Ticks every agent running the tree named by `F`, in query order.
fn tick_behaviors<C, A, F>(
    tree: Res<BehaviorTree<C, A, F>>,
    time: Option<Res<Time>>,
    mut agents: Agents<C, A, F>,
    mut commands: Commands,
) where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    let clock = clock(time.as_deref());
    for (entity, behavior, bb, held) in agents.iter_mut() {
        let change = tick_agent(&tree, clock(entity), behavior, bb, held);
        apply(change, entity, &mut commands);
    }
}

/// Ticks every agent running the tree named by `F` across the task pool.
fn tick_behaviors_parallel<C, A, F>(
    tree: Res<BehaviorTree<C, A, F>>,
    time: Option<Res<Time>>,
    mut agents: Agents<C, A, F>,
    commands: ParallelCommands,
) where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    let clock = clock(time.as_deref());
    agents
        .par_iter_mut()
        .for_each(|(entity, behavior, bb, held)| {
            let change = tick_agent(&tree, clock(entity), behavior, bb, held);
            // Only an act that appeared or went needs a command, and that is the
            // rare case: an agent that keeps doing the same kind of thing had
            // its act written in place above.
            if !matches!(change, ActChange::Settled) {
                commands.command_scope(|mut commands| apply(change, entity, &mut commands));
            }
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
fn tick_agent<C, A, F>(
    tree: &BehaviorTree<C, A, F>,
    at: TickAt,
    mut behavior: Mut<'_, Behavior<C, A, F>>,
    mut bb: Mut<'_, C>,
    held: Option<Mut<'_, A>>,
) -> ActChange<A>
where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    // Whether a blackboard changed is the gather's business, not the tick's: it
    // is rewritten every tick anyway, and marking the whole population changed
    // would drag the rest of the engine along. Nodes may write to it -- it is
    // how they leave notes for each other -- and those writes are *not* visible
    // to `Changed<C>` either. The blackboard is the tree's input; its output is
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
enum ActChange<A> {
    /// Written in place, or nothing to write.
    Settled,
    Appeared(A),
    Gone,
}

fn apply<A: Component>(change: ActChange<A>, entity: Entity, commands: &mut Commands) {
    match change {
        ActChange::Settled => {}
        ActChange::Appeared(act) => {
            commands.entity(entity).try_insert(act);
        }
        ActChange::Gone => {
            commands.entity(entity).try_remove::<A>();
        }
    }
}
