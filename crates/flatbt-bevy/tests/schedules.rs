//! Registration happens when the app is built, so any schedule will hold a tick.

use core::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use bevy_time::{Fixed, Time, TimePlugin};
use flatbt_bevy::prelude::*;

#[derive(Component, Default, Debug, PartialEq)]
struct Agent {
    fired: u32,
    has_turn: bool,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct Acting;

fn fire() -> impl BehaviorNode<Agent, Acting> {
    leaf(|agent: &mut Agent| {
        agent.fired += 1;
        NodeResult::Success
    })
}

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TurnPhase;

fn ticks_over_two_frames(schedule: impl ScheduleLabel) -> u32 {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(fire).in_schedule(schedule));
    let entity = app
        .world_mut()
        .spawn((Agent::default(), Behavior::for_tree(fire)))
        .id();
    app.update();
    app.update();
    app.world().get::<Agent>(entity).unwrap().fired
}

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
        .spawn((Agent::default(), Behavior::for_tree(fire)))
        .id();

    app.update();
    assert_eq!(app.world().get::<Agent>(entity).unwrap().fired, 0);

    app.world_mut().run_schedule(TurnPhase);
    app.world_mut().run_schedule(TurnPhase);
    assert_eq!(app.world().get::<Agent>(entity).unwrap().fired, 2);
}

/// `FixedUpdate` runs a whole number of times per frame, including none and
/// including several. A tick registered there follows it.
#[test]
fn a_fixed_schedule_ticks_as_often_as_it_runs() {
    let mut app = App::new();
    app.add_plugins(TimePlugin)
        .add_plugins(BehaviorPlugin::for_tree(fire).in_schedule(FixedUpdate));
    app.insert_resource(Time::<Fixed>::from_seconds(0.01));
    let entity = app
        .world_mut()
        .spawn((Agent::default(), Behavior::for_tree(fire)))
        .id();

    // No time has passed, so the fixed loop has nothing to run.
    app.update();
    assert_eq!(app.world().get::<Agent>(entity).unwrap().fired, 0);

    // Hand the fixed loop five steps' worth of time and take one frame.
    app.world_mut()
        .resource_mut::<Time<Fixed>>()
        .accumulate_overstep(Duration::from_millis(50));
    app.update();
    assert_eq!(
        app.world().get::<Agent>(entity).unwrap().fired,
        5,
        "five fixed steps in one frame, five ticks"
    );
}

/// A tree registered into a schedule nobody runs is silent. Pinned so the
/// silence is a decision rather than a surprise: a component means nothing
/// without a system, here as anywhere in Bevy.
#[test]
fn a_schedule_that_never_runs_is_silent() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(fire).in_schedule(TurnPhase));
    let entity = app
        .world_mut()
        .spawn((Agent::default(), Behavior::for_tree(fire)))
        .id();

    for _ in 0..5 {
        app.update();
    }

    assert_eq!(app.world().get::<Agent>(entity).unwrap().fired, 0);
}

// --- turn based --------------------------------------------------------------

/// Nothing gates the tick query itself: every entity carrying a blackboard and
/// a [`Behavior`] is visited. A turn-based game gates with [`Tick::Skip`],
/// which does not enter the tree and leaves a suspended invocation -- and the
/// act it is carrying -- exactly as they were.
#[derive(Resource)]
struct Turn {
    order: Vec<Entity>,
    holder: usize,
}

fn pass_the_turn(mut turn: ResMut<Turn>, mut agents: Query<(Entity, &mut Agent)>) {
    turn.holder = (turn.holder + 1) % turn.order.len();
    let holder = turn.order[turn.holder];
    for (entity, mut agent) in agents.iter_mut() {
        agent.has_turn = entity == holder;
    }
}

/// Runs over three turns, so the gate has to survive a suspended tree.
struct Act;

impl BtNode<Agent, Acting> for Act {
    type State = u32;

    fn update(
        &self,
        turns: &mut u32,
        agent: &mut Agent,
        _: (),
        _: Entry<'_>,
    ) -> NodeResult<Acting> {
        *turns += 1;
        agent.fired += 1;
        if *turns < 3 {
            NodeResult::Running(Acting)
        } else {
            NodeResult::Success
        }
    }
}

fn take_turn() -> impl BehaviorNode<Agent, Acting> {
    seq((Act,))
}

#[test]
fn agents_can_be_ticked_one_at_a_time_in_an_order_the_game_sets() {
    let mut app = App::new();
    app.add_plugins(
        BehaviorPlugin::for_tree(take_turn).tick_mode(|agent: &Agent, _| {
            if agent.has_turn {
                Tick::Resume
            } else {
                Tick::Skip
            }
        }),
    );
    let agents: Vec<Entity> = (0..3)
        .map(|_| {
            app.world_mut()
                .spawn((Agent::default(), Behavior::for_tree(take_turn)))
                .id()
        })
        .collect();
    app.insert_resource(Turn {
        holder: agents.len() - 1,
        order: agents.clone(),
    })
    .add_systems(Update, pass_the_turn.before(BehaviorSystems));

    for _ in 0..6 {
        app.update();
    }

    let fired: Vec<u32> = agents
        .iter()
        .map(|agent| app.world().get::<Agent>(*agent).unwrap().fired)
        .collect();
    assert_eq!(
        fired,
        vec![2, 2, 2],
        "each agent acted exactly on its own turns, and each resumed the \
         invocation it left rather than starting a new one"
    );
}
