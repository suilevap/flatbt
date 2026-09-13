//! Registration happens when the app is built, so any schedule will hold a tick.

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
