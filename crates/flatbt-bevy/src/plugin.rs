use bevy_app::{App, Plugin, Update};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};
use flatbt_core::EntryMode;

use crate::{Behavior, BehaviorTree, EntryModeFn, TreeBuilder};

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
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Component, Default)]
/// # struct Guard { ammo: u32 }
/// fn patrol() -> impl BehaviorNode<Guard> {
///     check(|guard: &Guard| guard.ammo > 0)
/// }
///
/// App::new().add_plugins(BehaviorPlugin::for_tree(patrol));
/// ```
pub struct BehaviorPlugin<C: Send + Sync + 'static, F: TreeBuilder<C>> {
    builder: F,
    entry_mode: EntryModeFn<C>,
    schedule: InternedScheduleLabel,
    parallel: bool,
}

impl<C: Component<Mutability = Mutable>, F: TreeBuilder<C>> BehaviorPlugin<C, F> {
    /// Builds the tree once and ticks its agents in [`Update`], one at a time,
    /// in query order. Both type parameters come from the builder.
    pub fn for_tree(builder: F) -> Self {
        Self {
            builder,
            entry_mode: |_| EntryMode::Evaluate,
            schedule: Update.intern(),
            parallel: false,
        }
    }

    /// Decides, per agent per tick, whether a suspended invocation reconsiders
    /// from the root or continues where it left off.
    ///
    /// Reconsidering is the default, because it is the answer that cannot be
    /// wrong: a tree that only ever resumes never leaves the branch it is in,
    /// so `select` never rescans and `choose!` never re-picks. Resuming is an
    /// optimisation, correct exactly when the standing decision is known to
    /// still hold -- and worth measuring before reaching for, because a tree
    /// whose invocations end each tick has nothing to resume into.
    ///
    /// The answer comes from the blackboard, so anything it needs -- a clock, a
    /// staggered slot, a perception flag -- is gathered like everything else.
    /// See [`evaluate_every`](crate::evaluate_every).
    pub fn entry_mode(mut self, entry_mode: EntryModeFn<C>) -> Self {
        self.entry_mode = entry_mode;
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
    /// A tree touches one component, its own blackboard, which is disjoint per
    /// entity, so this needs no further declaration. Iteration order becomes
    /// unspecified, which nothing in a tree can observe.
    pub fn parallel(mut self) -> Self {
        self.parallel = true;
        self
    }
}

impl<C: Component<Mutability = Mutable>, F: TreeBuilder<C>> Plugin for BehaviorPlugin<C, F> {
    fn build(&self, app: &mut App) {
        app.insert_resource(BehaviorTree::<C, F>::new(&self.builder, self.entry_mode));
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

/// The agents one tick system drives: their saved state and their blackboard.
type Agents<'w, 's, C, F> = Query<'w, 's, (&'static mut Behavior<C, F>, &'static mut C)>;

/// Ticks every agent running the tree named by `F`, in query order.
fn tick_behaviors<C: Component<Mutability = Mutable>, F: TreeBuilder<C>>(
    tree: Res<BehaviorTree<C, F>>,
    mut agents: Agents<C, F>,
) {
    let (definition, entry_mode) = (tree.get(), tree.entry_mode());
    for (mut behavior, mut bb) in agents.iter_mut() {
        // Whether a blackboard changed is the gather's business, not the tick's:
        // it is rewritten every tick anyway, and marking the whole population
        // changed would drag the rest of the engine along.
        let bb = bb.bypass_change_detection();
        behavior.tick(definition, bb, entry_mode(bb));
    }
}

/// Ticks every agent running the tree named by `F` across the task pool.
fn tick_behaviors_parallel<C: Component<Mutability = Mutable>, F: TreeBuilder<C>>(
    tree: Res<BehaviorTree<C, F>>,
    mut agents: Agents<C, F>,
) {
    let (definition, entry_mode) = (tree.get(), tree.entry_mode());
    agents.par_iter_mut().for_each(|(mut behavior, mut bb)| {
        let bb = bb.bypass_change_detection();
        behavior.tick(definition, bb, entry_mode(bb));
    });
}
