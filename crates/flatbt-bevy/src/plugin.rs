use core::marker::PhantomData;
use std::sync::Mutex;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::entity::{Entities, EntityAllocator};
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};
use bevy_ecs::system::{StaticSystemParam, SystemParamItem};
use bevy_ecs::world::{CommandQueue, DeferredWorld};

use crate::tree::EntryModeFn;
use crate::{
    AgentItem, Behavior, BehaviorContext, BehaviorTree, Blackboard, ParamItem, TreeBuilder,
    log_error,
};

/// Reports an agent whose tree was never registered.
///
/// Runs as `Behavior`'s `on_add` hook. Nothing else could: without a
/// registration no system queries this component type, so the agent would sit
/// there doing nothing, silently. Reported once per tree.
pub(crate) fn warn_unregistered<C: BehaviorContext, F: TreeBuilder<C>>(
    mut world: DeferredWorld<'_>,
    _: HookContext,
) {
    if world.get_resource::<BehaviorTree<C, F>>().is_some()
        || world.get_resource::<Reported<C, F>>().is_some()
    {
        return;
    }
    world
        .commands()
        .queue(|world: &mut World| world.insert_resource(Reported::<C, F>(PhantomData)));
    log_error(format_args!(
        "no tree registered for Behavior<{}, {}>: add BehaviorPlugin::for_tree with the same \
         builder, spelled the same way",
        core::any::type_name::<C>(),
        core::any::type_name::<F>(),
    ));
}

/// Set once a missing registration has been reported, to bound the noise.
#[derive(Resource)]
struct Reported<C: BehaviorContext, F: TreeBuilder<C>>(PhantomData<fn() -> (C, F)>);

/// All behavior ticks, whatever their tree. Order other systems against this.
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
    /// and [`Blackboard::commands`] are queued per batch, applied in batch
    /// completion order once every agent has ticked.
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

/// One command queue per batch of agents, handed back when the batch ends.
///
/// [`ParallelCommands`] takes a thread-local borrow per call, which at one call
/// per agent costs more than the tick it guards. A batch is the natural unit:
/// `for_each_init` builds one of these per batch and drops it at the end, so
/// the queue reaches the sink exactly once however the pool splits the work.
///
/// [`ParallelCommands`]: bevy_ecs::system::ParallelCommands
struct Batch<'a> {
    queue: CommandQueue,
    sink: &'a Mutex<Vec<CommandQueue>>,
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        if !self.queue.is_empty()
            && let Ok(mut sink) = self.sink.lock()
        {
            sink.push(core::mem::take(&mut self.queue));
        }
    }
}

/// Ticks every agent running the tree named by `F` across the task pool.
///
/// Registered by [`BehaviorPlugin::parallel`]. Iteration order is unspecified.
pub(crate) fn tick_behaviors_parallel<C: BehaviorContext, F: TreeBuilder<C>>(
    tree: Res<BehaviorTree<C, F>>,
    mut agents: Agents<C, F>,
    shared: StaticSystemParam<C::Param>,
    entities: &Entities,
    allocator: &EntityAllocator,
    mut commands: Commands,
) where
    for<'w, 's> SystemParamItem<'w, 's, C::Param>: Sync,
{
    let (entry_mode, shared) = (tree.entry_mode(), &*shared);
    let tree = tree.get();
    let sink = Mutex::new(Vec::new());
    agents.par_iter_mut().for_each_init(
        || Batch {
            queue: CommandQueue::default(),
            sink: &sink,
        },
        |batch, (entity, mut behavior, agent)| {
            tick_agent(
                entry_mode,
                tree,
                shared,
                entity,
                &mut behavior,
                agent,
                Commands::new_from_entities(&mut batch.queue, allocator, entities),
            );
        },
    );
    for queue in sink.into_inner().into_iter().flatten() {
        commands.append(&mut { queue });
    }
}
