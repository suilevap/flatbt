use std::sync::{Mutex, PoisonError};

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
/// // Whatever keeps the agent busy is what decides when to stop: while a node
/// // is running, no entry mode consults anything above it.
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
///
/// The blackboard is `Option<&mut C>` so that an agent that lost it is still
/// visited: it is not running the tree, and the tick has to take back whatever
/// it was last told to do.
type Agents<'w, 's, C, A, F> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Behavior<C, A, F>,
        Option<&'static mut C>,
        Option<&'static mut A>,
    ),
>;

/// One agent, as the tick query hands it over.
type Agent<'w, C, A, F> = (
    Entity,
    Mut<'w, Behavior<C, A, F>>,
    Option<Mut<'w, C>>,
    Option<Mut<'w, A>>,
);

/// Ticks every agent running the tree named by `F`, in query order.
fn tick_behaviors<C, A, F>(
    tree: Res<BehaviorTree<C, A, F>>,
    mut agents: Agents<C, A, F>,
    mut changes: Local<Changes<A>>,
    mut commands: Commands,
) where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    for agent in agents.iter_mut() {
        tick_agent(&tree, agent, &mut changes);
    }
    changes.flush(&mut commands);
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
    let shared = Mutex::new(Changes::new());
    agents.par_iter_mut().for_each_init(
        || Batch {
            local: Changes::new(),
            shared: &shared,
        },
        |batch, agent| tick_agent(&tree, agent, &mut batch.local),
    );
    let mut changes = shared.into_inner().unwrap_or_else(PoisonError::into_inner);
    changes.flush(&mut commands);
}

/// One agent's tick: enter the tree if it is running and the tick mode says to,
/// and record what that did to its act.
fn tick_agent<C, A, F>(
    tree: &BehaviorTree<C, A, F>,
    (entity, mut behavior, bb, held): Agent<'_, C, A, F>,
    changes: &mut Changes<A>,
) where
    C: Component<Mutability = Mutable>,
    A: Component<Mutability = Mutable> + PartialEq,
    F: TreeBuilder<C, A>,
{
    let Some(mut bb) = bb else {
        // No blackboard, no agent: it has nothing to decide from, so it decides
        // nothing and the order it was last given is taken back. The invocation
        // goes with it -- it was suspended in a world this agent no longer sees.
        if behavior.is_running() {
            behavior.restart();
        }
        if held.is_some() {
            changes.leaving.push(entity);
        }
        return;
    };
    // Whether a blackboard changed is the gather's business, not the tick's: it
    // is rewritten every tick anyway, and marking the whole population changed
    // would drag the rest of the engine along. Nodes may write to it -- it is
    // how they talk to each other -- and those writes are *not* visible to
    // `Changed<C>` either. The blackboard is the tree's input; its output is the
    // act.
    let bb = bb.bypass_change_detection();
    let Some(mode) = tree.tick_mode()(bb).entry_mode() else {
        // Skipped: the suspended invocation and the standing act stay as they
        // are, and the systems carrying that act out keep seeing it.
        return;
    };
    let decided = behavior.tick(tree.get(), bb, mode);
    match (decided, held) {
        // The common case by far: still doing something, so the component is
        // already there and only its contents can change.
        (Some(act), Some(mut held)) => {
            held.set_if_neq(act);
        }
        (Some(act), None) => changes.arriving.push((entity, act)),
        (None, Some(_)) => changes.leaving.push(entity),
        (None, None) => {}
    }
}

/// The acts a tick cannot write in place: one an agent did not have, and one an
/// agent stopped having. Both are structural, so both go through a command.
struct Changes<A> {
    arriving: Vec<(Entity, A)>,
    leaving: Vec<Entity>,
}

impl<A> Changes<A> {
    fn new() -> Self {
        Self {
            arriving: Vec::new(),
            leaving: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.arriving.is_empty() && self.leaving.is_empty()
    }

    fn append(&mut self, other: &mut Self) {
        self.arriving.append(&mut other.arriving);
        self.leaving.append(&mut other.leaving);
    }
}

impl<A: Component> Changes<A> {
    fn flush(&mut self, commands: &mut Commands) {
        if !self.arriving.is_empty() {
            commands.try_insert_batch(core::mem::take(&mut self.arriving));
        }
        for entity in self.leaving.drain(..) {
            commands.entity(entity).try_remove::<A>();
        }
    }
}

/// `Local<Changes<A>>` keeps the serial tick's buffers allocated between runs.
impl<A> Default for Changes<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// One parallel batch's changes, handed to the shared list when the batch ends.
///
/// The lock is taken in `drop` rather than per agent: `for_each_init` builds one
/// of these per batch and drops it when that batch is done, so a batch that
/// changed nothing -- the common case, since most agents keep doing what they
/// were doing -- never takes it at all.
struct Batch<'a, A> {
    local: Changes<A>,
    shared: &'a Mutex<Changes<A>>,
}

impl<A> Drop for Batch<'_, A> {
    fn drop(&mut self) {
        if self.local.is_empty() {
            return;
        }
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.append(&mut self.local);
    }
}
