use bevy_app::{App, Plugin, Update};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};

use crate::{Behavior, BehaviorTree, Tick, TickFn, TreeBuilder};

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
///     seq((
///         check(|guard: &Guard| guard.ammo > 0),
///         leaf(|_: &mut Guard| NodeResult::Running(Act::Firing)),
///     ))
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
            tick_mode: |_| Tick::Evaluate,
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
    /// The answer comes from the blackboard, so anything it needs -- a clock, a
    /// staggered slot, a perception flag, whose turn it is -- is gathered like
    /// everything else. See [`evaluate_every`](crate::evaluate_every) and
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
    /// BehaviorPlugin::for_tree(patrol).tick_mode(|guard: &Guard| {
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
    mut agents: Agents<C, A, F>,
    mut arriving: Local<Vec<(Entity, A)>>,
    mut leaving: Local<Vec<Entity>>,
    mut commands: Commands,
) where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    let (definition, tick_mode) = (tree.get(), tree.tick_mode());
    for (entity, mut behavior, mut bb, held) in agents.iter_mut() {
        // Whether a blackboard changed is the gather's business, not the tick's:
        // it is rewritten every tick anyway, and marking the whole population
        // changed would drag the rest of the engine along.
        let bb = bb.bypass_change_detection();
        let Some(mode) = tick_mode(bb).entry_mode() else {
            continue;
        };
        apply(
            entity,
            behavior.tick(definition, bb, mode),
            held,
            &mut arriving,
            &mut leaving,
        );
    }
    flush(&mut arriving, &mut leaving, &mut commands);
}

/// Ticks every agent running the tree named by `F` across the task pool.
fn tick_behaviors_parallel<C, A, F>(
    tree: Res<BehaviorTree<C, A, F>>,
    mut agents: Agents<C, A, F>,
    mut commands: Commands,
) where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    let (definition, tick_mode) = (tree.get(), tree.tick_mode());
    let changes = std::sync::Mutex::new((Vec::new(), Vec::new()));
    agents.par_iter_mut().for_each_init(
        || (Vec::new(), Vec::new()),
        |(arriving, leaving), (entity, mut behavior, mut bb, held)| {
            let bb = bb.bypass_change_detection();
            let Some(mode) = tick_mode(bb).entry_mode() else {
                return;
            };
            apply(
                entity,
                behavior.tick(definition, bb, mode),
                held,
                arriving,
                leaving,
            );
            // Only the rare agent whose act appeared or went needs a command,
            // so this lock is taken once per batch that had one rather than
            // once per agent.
            if !arriving.is_empty() || !leaving.is_empty() {
                let mut changes = changes.lock().expect("behavior tick panicked");
                changes.0.append(arriving);
                changes.1.append(leaving);
            }
        },
    );
    let (mut arriving, mut leaving) = changes.into_inner().expect("behavior tick panicked");
    flush(&mut arriving, &mut leaving, &mut commands);
}

/// Writes an act in place where it can be, and queues the rest.
fn apply<A: Component<Mutability = Mutable> + PartialEq>(
    entity: Entity,
    decided: Option<A>,
    held: Option<Mut<'_, A>>,
    arriving: &mut Vec<(Entity, A)>,
    leaving: &mut Vec<Entity>,
) {
    match (decided, held) {
        // The common case by far: still doing something, so the component is
        // already there and only its contents can change.
        (Some(act), Some(mut held)) => {
            held.set_if_neq(act);
        }
        (Some(act), None) => arriving.push((entity, act)),
        (None, Some(_)) => leaving.push(entity),
        (None, None) => {}
    }
}

fn flush<A: Component>(
    arriving: &mut Vec<(Entity, A)>,
    leaving: &mut Vec<Entity>,
    commands: &mut Commands,
) {
    if !arriving.is_empty() {
        commands.try_insert_batch(core::mem::take(arriving));
    }
    for entity in leaving.drain(..) {
        commands.entity(entity).try_remove::<A>();
    }
}
