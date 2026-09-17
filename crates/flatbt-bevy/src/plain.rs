//! Experiment: the blackboard as an ordinary component, and nothing else.
//!
//! No `BehaviorContext`, no `read`, no `write`, no `Blackboard`. A tree's
//! context is a component the game fills with whatever systems it likes, at
//! whatever rates it likes, and reads back the same way.

use core::marker::PhantomData;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{InternedScheduleLabel, ScheduleLabel};
use flatbt_core::{BtNode, EntryMode, NodeResult};

use crate::BehaviorSystems;

/// A tree over `C`, with one state type.
pub trait Mind<C>: BtNode<C, State = Self::Data> + Send + Sync + 'static {
    type Data: Default + Send + Sync + 'static;
}

impl<C, N, S> Mind<C> for N
where
    N: BtNode<C, State = S> + Send + Sync + 'static,
    S: Default + Send + Sync + 'static,
{
    type Data = S;
}

/// Names one tree and builds it once.
pub trait Builder<C>: Send + Sync + 'static {
    type Tree: Mind<C>;
    fn build(&self) -> Self::Tree;
}

impl<C, N, F> Builder<C> for F
where
    F: Fn() -> N + Send + Sync + 'static,
    N: Mind<C>,
{
    type Tree = N;

    fn build(&self) -> N {
        self()
    }
}

/// The one tree named by `F`.
#[derive(Resource)]
pub struct Definition<C: Send + Sync + 'static, F: Builder<C>> {
    tree: F::Tree,
    mode: fn(&C) -> EntryMode,
    context: PhantomData<fn() -> C>,
}

/// One agent's invocation state.
#[derive(Component)]
pub struct Runs<C: Send + Sync + 'static, F: Builder<C>> {
    state: Option<<F::Tree as Mind<C>>::Data>,
    builder: PhantomData<fn() -> (C, F)>,
}

impl<C: Send + Sync + 'static, F: Builder<C>> Runs<C, F> {
    pub fn for_tree(_builder: F) -> Self {
        Self {
            state: None,
            builder: PhantomData,
        }
    }
}

/// Builds one tree and ticks every agent whose `C` it drives.
pub struct MindPlugin<C: Send + Sync + 'static, F: Builder<C>> {
    builder: F,
    mode: fn(&C) -> EntryMode,
    schedule: InternedScheduleLabel,
    parallel: bool,
}

impl<C: Component, F: Builder<C>> MindPlugin<C, F> {
    pub fn for_tree(builder: F) -> Self {
        Self {
            builder,
            mode: |_| EntryMode::Evaluate,
            schedule: Update.intern(),
            parallel: false,
        }
    }

    pub fn entry_mode(mut self, mode: fn(&C) -> EntryMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn in_schedule(mut self, schedule: impl ScheduleLabel) -> Self {
        self.schedule = schedule.intern();
        self
    }

    pub fn parallel(mut self) -> Self {
        self.parallel = true;
        self
    }
}

impl<C: Component<Mutability = bevy_ecs::component::Mutable>, F: Builder<C>> Plugin
    for MindPlugin<C, F>
{
    fn build(&self, app: &mut App) {
        app.insert_resource(Definition::<C, F> {
            tree: self.builder.build(),
            mode: self.mode,
            context: PhantomData,
        });
        if self.parallel {
            app.add_systems(self.schedule, tick_parallel::<C, F>.in_set(BehaviorSystems));
        } else {
            app.add_systems(self.schedule, tick::<C, F>.in_set(BehaviorSystems));
        }
    }
}

fn run<C: Send + Sync + 'static, F: Builder<C>>(
    definition: &Definition<C, F>,
    runs: &mut Runs<C, F>,
    ctx: &mut C,
) {
    let mode = if runs.state.is_none() {
        EntryMode::Evaluate
    } else {
        (definition.mode)(ctx)
    };
    let result = definition.tree.update(
        runs.state.get_or_insert_with(Default::default),
        ctx,
        (),
        mode,
    );
    if result != NodeResult::Running {
        runs.state = None;
    }
    if result == NodeResult::Failure && mode == EntryMode::Resume {
        let again = definition.tree.update(
            runs.state.get_or_insert_with(Default::default),
            ctx,
            (),
            EntryMode::Evaluate,
        );
        if again != NodeResult::Running {
            runs.state = None;
        }
    }
}

fn tick<C: Component<Mutability = bevy_ecs::component::Mutable>, F: Builder<C>>(
    definition: Res<Definition<C, F>>,
    mut agents: Query<(&mut Runs<C, F>, &mut C)>,
) {
    for (mut runs, mut ctx) in agents.iter_mut() {
        run(&definition, &mut runs, ctx.bypass_change_detection());
    }
}

fn tick_parallel<C: Component<Mutability = bevy_ecs::component::Mutable>, F: Builder<C>>(
    definition: Res<Definition<C, F>>,
    mut agents: Query<(&mut Runs<C, F>, &mut C)>,
) {
    let definition = &*definition;
    agents.par_iter_mut().for_each(|(mut runs, mut ctx)| {
        run(definition, &mut runs, ctx.bypass_change_detection());
    });
}
