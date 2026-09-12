use core::marker::PhantomData;

use bevy_platform::collections::HashSet;

use core::any::TypeId;

use bevy_app::{App, First, Plugin, Update};
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel, Schedules};
use bevy_ecs::system::{ParallelCommands, StaticSystemParam, SystemParamItem};
use bevy_ecs::world::DeferredWorld;

use crate::tree::EntryModeFn;
use crate::{
    AgentItem, Behavior, BehaviorContext, BehaviorTree, Blackboard, ParamItem, TreeBuilder,
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
/// Registrations are applied from [`First`], because a schedule cannot be
/// extended while it runs. Any stage of `Main` after it can hold the tick;
/// [`First`] itself cannot, and is refused with a diagnostic. For a schedule
/// that runs outside `Main`, or before it, register trees explicitly with
/// [`BehaviorPlugin::for_tree`], which adds the system when the app is built
/// and so has no ordering to satisfy.
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
    ///
    /// Must run after [`First`], where registrations are applied.
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
        if self.schedule == REGISTRATION_SCHEDULE.intern() {
            log_error(format_args!(
                "FlatBtPlugin cannot tick in {:?}: that is where it applies registrations, and \
                 a schedule cannot be extended while it runs. Register trees for it explicitly \
                 with BehaviorPlugin::for_tree",
                REGISTRATION_SCHEDULE,
            ));
            return;
        }
        app.insert_resource(SelfRegistering {
            schedule: self.schedule,
            pending: Vec::new(),
            claimed: HashSet::default(),
        })
        // A schedule cannot be extended while it runs, so registrations are
        // applied from the first one of the frame, which precedes every other
        // stage of `Main` and so every schedule a tick can sensibly run in.
        .add_systems(
            REGISTRATION_SCHEDULE,
            apply_registrations.run_if(|state: Res<SelfRegistering>| !state.pending.is_empty()),
        );
    }
}

/// Where self-registration is applied: the first stage of the frame, so every
/// later stage of `Main` can hold the tick.
const REGISTRATION_SCHEDULE: First = First;

/// Trees whose tick system is registered but not yet added to a schedule.
#[derive(Resource)]
struct SelfRegistering {
    schedule: InternedScheduleLabel,
    pending: Vec<fn(&mut World)>,
    /// Trees whose build has been claimed, so only the first agent builds.
    claimed: HashSet<TypeId>,
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
    let Some(mut registering) = world.get_resource_mut::<SelfRegistering>() else {
        // Report before building: without the plugin the tree would be
        // discarded, and a builder may do real work or have side effects.
        log_error(format_args!(
            "no tree built for Behavior<{}, {}>: add FlatBtPlugin, or register this tree \
             with BehaviorPlugin::for_tree",
            core::any::type_name::<C>(),
            core::any::type_name::<F>(),
        ));
        return;
    };
    // Claim the tree before building it. Several first agents can be spawned
    // before the queued command runs, and each would otherwise build a tree
    // that is then dropped.
    if !registering
        .claimed
        .insert(TypeId::of::<BehaviorTree<C, F>>())
    {
        return;
    }
    let Some(behavior) = world.get::<Behavior<C, F>>(context.entity) else {
        return;
    };
    let tree = behavior.builder().build();
    world.commands().queue(move |world: &mut World| {
        world.insert_resource(BehaviorTree::<C, F>::new(tree, None));
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
///     check(|bb: &Blackboard<Guard>| bb.ammo.0 > 0)
/// }
///
/// App::new().add_plugins(BehaviorPlugin::for_tree(patrol));
/// ```
pub struct BehaviorPlugin<C: BehaviorContext, F: TreeBuilder<C>> {
    builder: F,
    schedule: InternedScheduleLabel,
    parallel: bool,
    entry_mode: Option<EntryModeFn<C>>,
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
            entry_mode: None,
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

    /// Overrides [`BehaviorContext::entry_mode`] for this tree alone.
    ///
    /// The context answers for a family of trees that share its access. Trees
    /// that share access but not pace -- a combat tree that rethinks often, a
    /// scripted one that never does -- set their own here.
    pub fn entry_mode(mut self, entry_mode: EntryModeFn<C>) -> Self {
        self.entry_mode = Some(entry_mode);
        self
    }

    /// Spreads agents across the task pool with [`Query::par_iter_mut`].
    ///
    /// Agent access is disjoint per entity and shared access is read-only, so
    /// this needs no further declaration. Iteration order becomes unspecified
    /// and [`Blackboard::commands`] are queued per worker thread.
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
        app.insert_resource(BehaviorTree::<C, F>::new(
            self.builder.build(),
            self.entry_mode,
        ));
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
>;

/// One agent's update, shared by both tick systems.
fn tick_agent<'w, 's, 'q, 'a, 'c, C: BehaviorContext, F: TreeBuilder<C>>(
    entry_mode: EntryModeFn<C>,
    tree: &F::Tree,
    shared: &'a ParamItem<'w, 's, C>,
    entity: Entity,
    behavior: &mut Behavior<C, F>,
    agent: AgentItem<'a, 'q, C>,
    commands: Commands<'c, 'c>,
) {
    let mut bb = Blackboard {
        entity,
        agent,
        shared,
        commands,
    };
    let mode = entry_mode(&bb);
    behavior.tick(tree, &mut bb, mode);
}

/// Ticks every agent running the tree named by `F`, in query order.
///
/// Registered by [`BehaviorPlugin`] and by self-registration. Not public:
/// `F` is a builder's own type, which no call site can name or infer, so
/// choose the schedule and tick mode through the plugin instead.
pub(crate) fn tick_behaviors<C: BehaviorContext, F: TreeBuilder<C>>(
    tree: Res<BehaviorTree<C, F>>,
    mut agents: Agents<C, F>,
    shared: StaticSystemParam<C::Param>,
    mut commands: Commands,
) {
    let (entry_mode, shared) = (tree.entry_mode(), &*shared);
    let tree = tree.get();
    for (entity, mut behavior, agent) in agents.iter_mut() {
        tick_agent(
            entry_mode,
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
/// Registered by [`BehaviorPlugin::parallel`]. Iteration order is unspecified.
pub(crate) fn tick_behaviors_parallel<C: BehaviorContext, F: TreeBuilder<C>>(
    tree: Res<BehaviorTree<C, F>>,
    mut agents: Agents<C, F>,
    shared: StaticSystemParam<C::Param>,
    par_commands: ParallelCommands,
) where
    for<'w, 's> SystemParamItem<'w, 's, C::Param>: Sync,
{
    let (entry_mode, shared) = (tree.entry_mode(), &*shared);
    let tree = tree.get();
    agents
        .par_iter_mut()
        .for_each(|(entity, mut behavior, agent)| {
            par_commands.command_scope(|commands| {
                tick_agent(
                    entry_mode,
                    tree,
                    shared,
                    entity,
                    &mut behavior,
                    agent,
                    commands,
                );
            });
        });
}
