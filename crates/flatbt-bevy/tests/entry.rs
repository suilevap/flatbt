//! What each [`Tick`] does to an agent, and to the act it is carrying.
//!
//! The property that matters is first: `Evaluate` every tick is always correct,
//! and the other two only cost responsiveness.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;

#[derive(Component, Default, Debug, PartialEq)]
struct Work {
    done: u32,
    /// Written by a gather, so the tick mode does not gate on the progress it
    /// is gating -- which deadlocks, and is worth remembering.
    frame: u32,
    awake: bool,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct Working;

/// Three steps of work, then done.
struct ThreeSteps;

impl BtNode<Work, Working> for ThreeSteps {
    type State = u32;

    fn update(&self, step: &mut u32, work: &mut Work, _: (), _: Entry<'_>) -> NodeResult<Working> {
        *step += 1;
        work.done += 1;
        if *step < 3 {
            NodeResult::Running(Working)
        } else {
            NodeResult::Success
        }
    }
}

fn steps() -> impl BehaviorNode<Work, Working> {
    seq((ThreeSteps,))
}

fn count_frames(mut agents: Query<&mut Work>) {
    for mut work in agents.iter_mut() {
        work.bypass_change_detection().frame += 1;
    }
}

fn run(mode: flatbt_bevy::TickFn<Work>, ticks: u32) -> Work {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(steps).tick_mode(mode))
        .add_systems(Update, count_frames.before(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((Work::default(), Behavior::for_tree(steps)))
        .id();
    for _ in 0..ticks {
        app.update();
    }
    app.world_mut().entity_mut(agent).take::<Work>().unwrap()
}

/// `Evaluate` every tick is the answer that cannot be wrong. `Resume` and
/// `Skip` are optimisations: they change how many ticks an agent needs, never
/// where it gets to.
#[test]
fn every_tick_mode_reaches_the_same_place_given_enough_ticks() {
    assert_eq!(run(|_, _| Tick::Evaluate, 3).done, 3);
    assert_eq!(run(|_, _| Tick::Resume, 3).done, 3);
    assert_eq!(
        run(
            |work: &Work, _| {
                if work.frame.is_multiple_of(2) {
                    Tick::Evaluate
                } else {
                    Tick::Skip
                }
            },
            6
        )
        .done,
        3,
        "skipping costs ticks, not progress"
    );
}

/// `Skip` leaves the standing act alone, which is the whole point of it: the
/// systems carrying that act out keep seeing it.
#[test]
fn skip_leaves_the_standing_act_in_place() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(steps).tick_mode(|work: &Work, _| {
        if work.awake { Tick::Resume } else { Tick::Skip }
    }));
    let agent = app
        .world_mut()
        .spawn((
            Work {
                awake: true,
                ..Work::default()
            },
            Behavior::for_tree(steps),
        ))
        .id();

    app.update();
    assert_eq!(app.world().get::<Working>(agent), Some(&Working));
    assert_eq!(app.world().get::<Work>(agent).unwrap().done, 1);

    // Asleep: the tree is not entered, and what it decided last still stands.
    app.world_mut().get_mut::<Work>(agent).unwrap().awake = false;
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(
        app.world().get::<Work>(agent).unwrap().done,
        1,
        "not ticked"
    );
    assert_eq!(
        app.world().get::<Working>(agent),
        Some(&Working),
        "the standing act is untouched, so systems acting on it carry on"
    );

    // Awake again: it resumes into the node it was in.
    app.world_mut().get_mut::<Work>(agent).unwrap().awake = true;
    app.update();
    assert_eq!(app.world().get::<Work>(agent).unwrap().done, 2);
}

/// An agent skipped before its first tick never decided anything, so it carries
/// no act -- `Skip` preserves a standing decision, it does not invent one.
#[test]
fn skip_before_the_first_tick_leaves_the_agent_with_nothing() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(steps).tick_mode(|_, _| Tick::Skip));
    let agent = app
        .world_mut()
        .spawn((Work::default(), Behavior::for_tree(steps)))
        .id();

    for _ in 0..3 {
        app.update();
    }

    assert_eq!(app.world().get::<Work>(agent).unwrap().done, 0);
    assert_eq!(app.world().get::<Working>(agent), None);
}

/// Stopping every tree at once is a run condition, and it also freezes the acts.
#[test]
fn a_run_condition_stops_the_tick_and_freezes_what_agents_are_doing() {
    #[derive(Resource, Default)]
    struct Paused(bool);

    let mut app = App::new();
    app.init_resource::<Paused>()
        .add_plugins(BehaviorPlugin::for_tree(steps).tick_mode(|_, _| Tick::Resume))
        .configure_sets(
            Update,
            BehaviorSystems.run_if(|paused: Res<Paused>| !paused.0),
        );
    let agent = app
        .world_mut()
        .spawn((Work::default(), Behavior::for_tree(steps)))
        .id();

    app.update();
    assert_eq!(app.world().get::<Working>(agent), Some(&Working));

    app.world_mut().resource_mut::<Paused>().0 = true;
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(app.world().get::<Work>(agent).unwrap().done, 1);
    assert_eq!(app.world().get::<Working>(agent), Some(&Working));
}

#[test]
fn skip_is_the_one_tick_that_does_not_enter_the_tree() {
    assert_eq!(Tick::Skip.entry_mode(), None);
    assert_eq!(Tick::Resume.entry_mode(), Some(EntryMode::Resume));
    assert_eq!(Tick::Evaluate.entry_mode(), Some(EntryMode::Evaluate));
}
