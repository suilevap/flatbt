//! Where self-registration can put a tick, and where it cannot.

use core::sync::atomic::{AtomicUsize, Ordering};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use bevy_ecs::schedule::ScheduleLabel;
use flatbt_bevy::prelude::*;

#[derive(Component, Debug, PartialEq)]
struct Fired(u32);

#[derive(QueryData)]
#[query_data(mutable)]
struct Agent {
    fired: &'static mut Fired,
}

impl BehaviorContext for Agent {
    type Agent = Self;
    type Param = ();
}

fn fire() -> impl BehaviorNode<Agent> {
    leaf(|bb: &mut Blackboard<Agent>| {
        bb.fired.0 += 1;
        NodeResult::Success
    })
}

fn ticks_over_two_frames(schedule: impl ScheduleLabel) -> u32 {
    let mut app = App::new();
    app.add_plugins(FlatBtPlugin::new().in_schedule(schedule));
    let entity = app
        .world_mut()
        .spawn((Fired(0), Behavior::for_tree(fire)))
        .id();
    app.update();
    app.update();
    app.world().get::<Fired>(entity).unwrap().0
}

/// Registrations are applied from `First`, so every later stage of `Main` can
/// hold the tick, including one that runs before `Update`.
#[test]
fn self_registration_reaches_every_stage_after_first() {
    assert_eq!(ticks_over_two_frames(PreUpdate), 2);
    assert_eq!(ticks_over_two_frames(Update), 2);
    assert_eq!(ticks_over_two_frames(PostUpdate), 2);
    assert_eq!(ticks_over_two_frames(Last), 2);
}

/// `First` is where registrations are applied, and a schedule cannot be extended
/// while it runs. The plugin refuses rather than silently never ticking.
#[test]
fn self_registration_refuses_its_own_schedule() {
    assert_eq!(ticks_over_two_frames(First), 0);
}

// A counter per test: the suite runs tests in parallel, so a shared one would
// be read across them.
static SHARED_BUILDS: AtomicUsize = AtomicUsize::new(0);
static ORPHAN_BUILDS: AtomicUsize = AtomicUsize::new(0);

fn counted_shared() -> impl BehaviorNode<Agent> {
    SHARED_BUILDS.fetch_add(1, Ordering::Relaxed);
    fire()
}

fn counted_orphan() -> impl BehaviorNode<Agent> {
    ORPHAN_BUILDS.fetch_add(1, Ordering::Relaxed);
    fire()
}

#[test]
fn a_tree_is_built_once_however_many_agents_arrive_together() {
    let mut app = App::new();
    app.add_plugins(FlatBtPlugin::new());

    // All eight are spawned before any queued command runs.
    let world = app.world_mut();
    for _ in 0..8 {
        world.spawn((Fired(0), Behavior::for_tree(counted_shared)));
    }
    app.update();

    assert_eq!(SHARED_BUILDS.load(Ordering::Relaxed), 1);
}

#[test]
fn no_plugin_means_the_builder_is_never_run() {
    let mut app = App::new();
    app.world_mut()
        .spawn((Fired(0), Behavior::for_tree(counted_orphan)));
    app.update();

    assert_eq!(ORPHAN_BUILDS.load(Ordering::Relaxed), 0);
}
