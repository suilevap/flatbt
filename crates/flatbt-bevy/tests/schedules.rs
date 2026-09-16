//! Registration happens when the app is built, so any schedule will hold a tick.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use bevy_ecs::schedule::ScheduleLabel;
use flatbt_bevy::prelude::*;

#[derive(Component, Debug, PartialEq)]
struct Fired(u32);

struct Agent {
    fired: u32,
}

#[derive(QueryData)]
#[query_data(mutable)]
struct AgentAccess {
    fired: &'static mut Fired,
}

impl BehaviorContext for Agent {
    type Agent = AgentAccess;
    type Param = ();
    type Snapshot = Self;

    fn read(_: Entity, agent: &AgentAccessItem, _: &()) -> Agent {
        Agent {
            fired: agent.fired.0,
        }
    }

    fn write(agent: &Agent, access: &mut AgentAccessItem) {
        access.fired.set_if_neq(Fired(agent.fired));
    }
}

fn fire() -> impl BehaviorNode<Agent> {
    leaf(|bb: &mut Blackboard<Agent>| {
        bb.fired += 1;
        NodeResult::Success
    })
}

fn ticks_over_two_frames(schedule: impl ScheduleLabel) -> u32 {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(fire).in_schedule(schedule));
    let entity = app
        .world_mut()
        .spawn((Fired(0), Behavior::for_tree(fire)))
        .id();
    app.update();
    app.update();
    app.world().get::<Fired>(entity).unwrap().0
}

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TurnPhase;

#[test]
fn every_stage_of_main_can_hold_a_tick() {
    assert_eq!(ticks_over_two_frames(First), 2);
    assert_eq!(ticks_over_two_frames(PreUpdate), 2);
    assert_eq!(ticks_over_two_frames(Update), 2);
    assert_eq!(ticks_over_two_frames(PostUpdate), 2);
    assert_eq!(ticks_over_two_frames(Last), 2);
}

/// Including one the game runs itself, whenever it likes.
#[test]
fn a_schedule_the_game_runs_itself_holds_a_tick() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(fire).in_schedule(TurnPhase));
    let entity = app
        .world_mut()
        .spawn((Fired(0), Behavior::for_tree(fire)))
        .id();

    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));

    app.world_mut().run_schedule(TurnPhase);
    app.world_mut().run_schedule(TurnPhase);
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(2)));
}

/// `FixedUpdate` runs a whole number of times per frame, including none and
/// including several. A tick registered there follows it.
#[test]
fn a_fixed_schedule_ticks_as_often_as_it_runs() {
    use bevy_time::{Fixed, Time, TimePlugin};
    use core::time::Duration;

    let mut app = App::new();
    app.add_plugins(TimePlugin)
        .add_plugins(BehaviorPlugin::for_tree(fire).in_schedule(FixedUpdate));
    app.insert_resource(Time::<Fixed>::from_seconds(0.01));
    let entity = app
        .world_mut()
        .spawn((Fired(0), Behavior::for_tree(fire)))
        .id();

    // No time has passed, so the fixed loop has nothing to run.
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));

    // Hand the fixed loop five steps' worth of time and take one frame.
    app.world_mut()
        .resource_mut::<Time<Fixed>>()
        .accumulate_overstep(Duration::from_millis(50));
    app.update();
    assert_eq!(
        app.world().get::<Fired>(entity),
        Some(&Fired(5)),
        "five fixed steps in one frame, five ticks"
    );
}

/// Commands a node defers are applied by the schedule that ran the tick, not by
/// `Update`, so a tree ticking somewhere unusual still gets its edits.
#[test]
fn deferred_edits_land_in_the_schedule_that_ticked() {
    #[derive(Component)]
    struct Marked;

    fn mark() -> impl BehaviorNode<Agent> {
        leaf(|bb: &mut Blackboard<Agent>| {
            bb.agent_commands().insert(Marked);
            NodeResult::Success
        })
    }

    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(mark).in_schedule(TurnPhase));
    let entity = app
        .world_mut()
        .spawn((Fired(0), Behavior::for_tree(mark)))
        .id();

    app.world_mut().run_schedule(TurnPhase);
    assert!(
        app.world().get::<Marked>(entity).is_some(),
        "the insert applied when TurnPhase finished, with no Update in between"
    );
}

/// A tree registered into a schedule nobody runs is silent: no tick, no
/// warning. The registration is what the component hook checks for, and it is
/// there. Pinned so the silence is a decision rather than a surprise.
#[test]
fn a_schedule_that_never_runs_is_silent() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(fire).in_schedule(TurnPhase));
    let entity = app
        .world_mut()
        .spawn((Fired(0), Behavior::for_tree(fire)))
        .id();

    for _ in 0..5 {
        app.update();
    }
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));
}
