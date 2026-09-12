use core::marker::PhantomData;

use bevy_app::{App, Plugin, PreUpdate, Update};
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel, Schedules};
use bevy_ecs::system::{ParallelCommands, StaticSystemParam, SystemParamItem};
use bevy_ecs::world::DeferredWorld;

use crate::{
    AgentItem, Behavior, BehaviorContext, BehaviorPaused, BehaviorTree, Bt, ParamItem, TreeBuilder,
    log_error,
};

/// Builds and ticks every tree an agent asks for, with no registration per tree.
///
/// Add it once. The first agent to name a tree builds it and registers its tick
/// in [`Update`], or in the schedule given to [`in_schedule`](Self::in_schedule).
/// An agent spawned before the tick schedule runs — in `Startup`, say — ticks on
/// that same frame; one spawned from inside it starts on the next, after which
/// its tree is registered and later agents tick immediately.
///
/// Add [`BehaviorPlugin::for_tree`] as well for any tree that needs its own
/// schedule, ordering, run conditions, or the parallel tick; an explicitly
/// registered tree is left alone.
///
/// ```no_run
/// # use bevy_app::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// App::new().add_plugins(FlatBtPlugin::new());
/// ```
pub struct FlatBtPlugin {
    schedule: InternedScheduleLabel,
}

impl FlatBtPlugin {
    pub fn new() -> Self {
        Self {
            schedule: Update.intern(),
        }
    }

    /// Ticks self-registered trees in `schedule` instead of [`Update`].
    pub fn in_schedule(mut self, schedule: impl ScheduleLabel) -> Self {
        self.schedule = schedule.intern();
        self
    }
}

impl Default for FlatBtPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for FlatBtPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SelfRegistering {
            schedule: self.schedule,
            pending: Vec::new(),
        })
        // A schedule cannot be extended while it runs, so registrations are
        // applied from an earlier one.
        .add_systems(
            PreUpdate,
            apply_registrations.run_if(|state: Res<SelfRegistering>| !state.pending.is_empty()),
        );
    }
}

/// Trees whose tick system is registered but not yet added to a schedule.
#[derive(Resource)]
struct SelfRegistering {
    schedule: InternedScheduleLabel,
    pending: Vec<fn(&mut World)>,
}

fn apply_registrations(world: &mut World) {
    let pending = core::mem::take(&mut world.resource_mut::<SelfRegistering>().pending);
    for register in pending {
        register(world);
    }
}

fn add_tick<C: BehaviorContext, F: TreeBuilder<C>>(world: &mut World) {
    let schedule = world.resource::<SelfRegistering>().schedule;
    world
        .resource_mut::<Schedules>()
        .add_systems(schedule, tick_behaviors::<C, F>.in_set(BehaviorSystems));
}

/// Builds an agent's tree the first time one is seen, through [`FlatBtPlugin`].
///
/// Runs as `Behavior`'s `on_add` hook. Without either plugin nothing would tick
/// the agent, and nothing else would report it: its component type is one no
/// system queries.
pub(crate) fn request_registration<C: BehaviorContext, F: TreeBuilder<C>>(
    mut world: DeferredWorld<'_>,
    context: HookContext,
) {
    if world.get_resource::<BehaviorTree<C, F>>().is_some() {
        return;
    }
    let Some(behavior) = world.get::<Behavior<C, F>>(context.entity) else {
        return;
    };
    let tree = behavior.builder().build();
    if world.get_resource::<SelfRegistering>().is_none() {
        log_error(format_args!(
            "no tree built for Behavior<{}, {}>: add FlatBtPlugin, or register this tree \
             with BehaviorPlugin::for_tree",
            core::any::type_name::<C>(),
            core::any::type_name::<F>(),
        ));
        return;
    }
    world.commands().queue(move |world: &mut World| {
        if world.contains_resource::<BehaviorTree<C, F>>() {
            return;
        }
        world.insert_resource(BehaviorTree::<C, F>::new(tree));
        world
            .resource_mut::<SelfRegistering>()
            .pending
            .push(add_tick::<C, F>);
    });
}

/// All behavior ticks, whatever their tree. Order other systems against this.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BehaviorSystems;

/// Builds one tree ahead of its agents, with a schedule and tick mode of its own.
///
/// Only needed for a tree that wants something other than the defaults
/// [`FlatBtPlugin`] applies: a different schedule, ordering, run conditions, or
/// the parallel tick. Adding the same builder twice is rejected by Bevy as a
/// duplicate plugin.
///
/// ```no_run
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::query::QueryData;
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Component)]
/// # struct Ammo(u32);
/// # #[derive(QueryData)]
/// # #[query_data(mutable)]
/// # struct Guard { ammo: &'static mut Ammo }
/// # impl BehaviorContext for Guard { type Agent = Self; type Param = (); }
/// fn patrol() -> impl BehaviorNode<Guard> {
///     check(|bt: &Bt<Guard>| bt.ammo.0 > 0)
/// }
///
/// App::new().add_plugins(BehaviorPlugin::for_tree(patrol));
/// ```
pub struct BehaviorPlugin<C: BehaviorContext, F: TreeBuilder<C>> {
    builder: F,
    schedule: InternedScheduleLabel,
    parallel: bool,
    context: PhantomData<fn() -> C>,
}

impl<C: BehaviorContext, F: TreeBuilder<C>> BehaviorPlugin<C, F> {
    /// Builds the tree once and ticks its agents in [`Update`], one at a time,
    /// in query order. Both type parameters come from the builder.
    pub fn for_tree(builder: F) -> Self {
        Self {
            builder,
            schedule: Update.intern(),
            parallel: false,
            context: PhantomData,
        }
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
    /// Agent access is disjoint per entity and shared access is read-only, so
    /// this needs no further declaration. Iteration order becomes unspecified
    /// and [`Bt::commands`] are queued per worker thread.
    pub fn parallel(mut self) -> Self {
        self.parallel = true;
        self
    }
}

impl<C: BehaviorContext, F: TreeBuilder<C>> Plugin for BehaviorPlugin<C, F>
where
    for<'w, 's> SystemParamItem<'w, 's, C::Param>: Sync,
{
    fn build(&self, app: &mut App) {
        app.insert_resource(BehaviorTree::<C, F>::new(self.builder.build()));
        if self.parallel {
            app.add_systems(
                self.schedule,
                tick_behaviors_parallel::<C, F>.in_set(BehaviorSystems),
            );
        } else {
            app.add_systems(
                self.schedule,
                tick_behaviors::<C, F>.in_set(BehaviorSystems),
            );
        }
    }
}

/// The agents one tick system drives: their saved state and the agent view.
type Agents<'w, 's, C, F> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Behavior<C, F>,
        <C as BehaviorContext>::Agent,
    ),
    Without<BehaviorPaused>,
>;

/// Every agent of this tree the tick is responsible for, matched or not.
type Owners<'w, 's, C, F> = Query<'w, 's, (), (With<Behavior<C, F>>, Without<BehaviorPaused>)>;

/// One agent's update, shared by both tick systems.
fn tick_agent<'w, 's, 'q, 'a, 'c, C: BehaviorContext, F: TreeBuilder<C>>(
    tree: &F::Tree,
    shared: &'a ParamItem<'w, 's, C>,
    entity: Entity,
    behavior: &mut Behavior<C, F>,
    agent: AgentItem<'a, 'q, C>,
    commands: Commands<'c, 'c>,
) {
    let mut bt = Bt {
        entity,
        agent,
        shared,
        commands,
    };
    let _ = behavior.tick(tree, &mut bt);
}

/// Ticks every agent running the tree named by `F`, in query order.
///
/// Added by [`BehaviorPlugin`] and by self-registration. Add it directly to
/// place the tick in a custom set or schedule; it needs [`BehaviorTree<C, F>`]
/// in the world.
pub fn tick_behaviors<C: BehaviorContext, F: TreeBuilder<C>>(
    tree: Res<BehaviorTree<C, F>>,
    mut agents: Agents<C, F>,
    all: Owners<C, F>,
    shared: StaticSystemParam<C::Param>,
    mut commands: Commands,
    reported: Local<bool>,
) {
    report_skipped::<C, F>(&all, || agents.iter().count(), reported);
    let (tree, shared) = (tree.get(), &*shared);
    for (entity, mut behavior, agent) in agents.iter_mut() {
        tick_agent(
            tree,
            shared,
            entity,
            &mut behavior,
            agent,
            commands.reborrow(),
        );
    }
}

/// Ticks every agent running the tree named by `F` across the task pool.
///
/// Added by [`BehaviorPlugin::parallel`]. Iteration order is unspecified.
pub fn tick_behaviors_parallel<C: BehaviorContext, F: TreeBuilder<C>>(
    tree: Res<BehaviorTree<C, F>>,
    mut agents: Agents<C, F>,
    all: Owners<C, F>,
    shared: StaticSystemParam<C::Param>,
    par_commands: ParallelCommands,
    reported: Local<bool>,
) where
    for<'w, 's> SystemParamItem<'w, 's, C::Param>: Sync,
{
    report_skipped::<C, F>(&all, || agents.iter().count(), reported);
    let (tree, shared) = (tree.get(), &*shared);
    agents
        .par_iter_mut()
        .for_each(|(entity, mut behavior, agent)| {
            par_commands.command_scope(|commands| {
                tick_agent(tree, shared, entity, &mut behavior, agent, commands);
            });
        });
}

/// Reports entities that own a behavior but do not match the agent query.
/// Debug builds only, and once per system, since the cause is a fixed mismatch.
fn report_skipped<C: BehaviorContext, F: TreeBuilder<C>>(
    all: &Owners<C, F>,
    ticked: impl FnOnce() -> usize,
    mut reported: Local<bool>,
) {
    if cfg!(debug_assertions) && !*reported {
        let (total, ticked) = (all.iter().count(), ticked());
        if total > ticked {
            *reported = true;
            let missing = total - ticked;
            log_error(format_args!(
                "skipping Behavior<{}, {}> on {missing} entities that do not match its \
                 BehaviorContext::Agent query",
                core::any::type_name::<C>(),
                core::any::type_name::<F>(),
            ));
        }
    }
}
