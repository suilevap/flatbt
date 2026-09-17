//! Ticking agents: what the plugin registers and what a tree can do to a
//! blackboard.

use core::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;

/// The blackboard: what the tree reads, and what it decided.
#[derive(Component, Default, Debug, PartialEq)]
struct Guard {
    ammo: u32,
    alarm: bool,
    fired: u32,
    reload: bool,
}

fn shoot() -> impl BehaviorNode<Guard> {
    seq((
        check(|guard: &Guard| guard.alarm),
        check(|guard: &Guard| guard.ammo > 0),
        leaf(|guard: &mut Guard| {
            guard.ammo -= 1;
            guard.fired += 1;
            NodeResult::Success
        }),
    ))
}

fn app<F: TreeBuilder<Guard>>(builder: F) -> App {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(builder));
    app
}

#[test]
fn a_tree_reads_and_writes_the_blackboard_until_it_fails() {
    let mut app = app(shoot);
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 2,
                alarm: true,
                ..Guard::default()
            },
            Behavior::for_tree(shoot),
        ))
        .id();

    for _ in 0..3 {
        app.update();
    }

    let guard = app.world().get::<Guard>(agent).unwrap();
    assert_eq!((guard.ammo, guard.fired), (0, 2), "two rounds, two shots");
}

#[test]
fn an_agent_without_the_blackboard_is_skipped() {
    let mut app = app(shoot);
    let bare = app.world_mut().spawn(Behavior::for_tree(shoot)).id();

    app.update();

    assert!(app.world().get::<Guard>(bare).is_none());
}

// --- state that survives a tick ----------------------------------------------

#[derive(Component, Default, Debug, PartialEq)]
struct Runner {
    steps: u32,
    patience: u32,
    gave_up: bool,
}

/// Runs for three updates, then succeeds.
struct March;

impl BtNode<Runner> for March {
    type State = u32;

    fn update(&self, done: &mut u32, runner: &mut Runner, _: (), _: EntryMode) -> NodeResult {
        *done += 1;
        runner.steps += 1;
        if *done < 3 {
            NodeResult::Running
        } else {
            NodeResult::Success
        }
    }
}

fn march() -> impl BehaviorNode<Runner> {
    seq((March,))
}

#[test]
fn a_suspended_invocation_resumes_where_it_left_off() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(march).tick_mode(|_| Tick::Resume));
    let agent = app
        .world_mut()
        .spawn((Runner::default(), Behavior::for_tree(march)))
        .id();

    for _ in 0..3 {
        app.update();
    }

    assert_eq!(
        app.world().get::<Runner>(agent).unwrap().steps,
        3,
        "one step per update, resuming into the same node"
    );

    // That invocation ended, so the fourth update starts a new one rather than
    // resuming a finished march.
    app.update();
    assert_eq!(app.world().get::<Runner>(agent).unwrap().steps, 4);
}

// --- a resumed tick that fails outright --------------------------------------

/// Runs while `patience` holds out, then fails.
struct Persist;

impl BtNode<Runner> for Persist {
    type State = ();

    fn update(&self, _: &mut (), runner: &mut Runner, _: (), _: EntryMode) -> NodeResult {
        if runner.patience > 0 {
            runner.patience -= 1;
            NodeResult::Running
        } else {
            NodeResult::Failure
        }
    }
}

/// The branch that persists is last, so when it fails there is nothing below it
/// for the selector to fall through to and the whole tree fails.
fn persist_or_give_up() -> impl BehaviorNode<Runner> {
    select((
        seq((check(|runner: &Runner| runner.gave_up), give_up())),
        Persist,
    ))
}

fn give_up() -> impl BehaviorNode<Runner> {
    leaf(|runner: &mut Runner| {
        runner.steps += 1;
        NodeResult::Success
    })
}

/// A `Resume` that fails at the root is re-entered once as `Evaluate`, so a
/// standing decision running out costs no idle tick.
///
/// This is the tick's doing, not the selector's: `Resume` stays an honest
/// resume, which never reconsiders a child above the one it saved.
#[test]
fn a_resumed_tick_that_fails_at_the_root_reconsiders_in_the_same_tick() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(persist_or_give_up).tick_mode(|_| Tick::Resume));
    let agent = app
        .world_mut()
        .spawn((
            Runner {
                patience: 1,
                // Set from the start: a gather would write it, and the point is
                // what the selector does with a child *above* the resumed one.
                gave_up: true,
                ..Runner::default()
            },
            Behavior::for_tree(persist_or_give_up),
        ))
        .id();

    // First update evaluates, so the selector scans from the top: the first
    // branch is skipped only because `Persist` is reached after it fails...
    app.update();
    assert_eq!(
        app.world().get::<Runner>(agent).unwrap().steps,
        1,
        "the first branch succeeds outright while it can"
    );

    // ...so make the first branch unavailable and let the resume stick.
    app.world_mut().get_mut::<Runner>(agent).unwrap().gave_up = false;
    app.update();
    assert_eq!(
        app.world().get::<Runner>(agent).unwrap().patience,
        0,
        "the tree is now suspended inside Persist"
    );

    // Persist fails this update. A plain resume would leave the agent idle,
    // because the branch above was never consulted; the retry consults it.
    app.world_mut().get_mut::<Runner>(agent).unwrap().gave_up = true;
    app.update();
    assert_eq!(
        app.world().get::<Runner>(agent).unwrap().steps,
        2,
        "the failed resume was re-entered as Evaluate in the same tick"
    );
}

#[test]
fn only_the_invocation_state_lives_on_the_agent() {
    assert_eq!(
        core::mem::size_of::<Behavior<Runner, fn() -> March>>(),
        core::mem::size_of::<Option<u32>>(),
        "no tree, no builder value, nothing but the state"
    );
}

// --- many agents, one tree ---------------------------------------------------

#[test]
fn one_tree_serves_many_agents_in_parallel() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(shoot).parallel());
    let agents: Vec<Entity> = (0..256)
        .map(|index| {
            app.world_mut()
                .spawn((
                    Guard {
                        ammo: index % 4,
                        alarm: true,
                        ..Guard::default()
                    },
                    Behavior::for_tree(shoot),
                ))
                .id()
        })
        .collect();

    app.update();

    for (index, agent) in agents.iter().enumerate() {
        let guard = app.world().get::<Guard>(*agent).unwrap();
        let expected = u32::from(!(index as u32).is_multiple_of(4));
        assert_eq!(guard.fired, expected, "agent {index} kept its own state");
    }
}

// --- two trees over one blackboard -------------------------------------------

fn hoard() -> impl BehaviorNode<Guard> {
    leaf(|guard: &mut Guard| {
        guard.reload = true;
        NodeResult::Success
    })
}

#[test]
fn two_trees_over_one_blackboard_each_get_their_own_tick() {
    let mut app = App::new();
    app.add_plugins((
        BehaviorPlugin::for_tree(shoot),
        BehaviorPlugin::for_tree(hoard),
    ));
    let shooter = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 1,
                alarm: true,
                ..Guard::default()
            },
            Behavior::for_tree(shoot),
        ))
        .id();
    let hoarder = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(hoard)))
        .id();

    app.update();

    assert_eq!(app.world().get::<Guard>(shooter).unwrap().fired, 1);
    assert!(app.world().get::<Guard>(hoarder).unwrap().reload);
    assert!(
        !app.world().get::<Guard>(shooter).unwrap().reload,
        "each agent ran only its own tree"
    );
}

/// Two names for one tree type, which is why the builder is the identity.
fn advance(step: u32) -> impl BehaviorNode<Guard> {
    leaf(move |guard: &mut Guard| {
        guard.ammo += step;
        NodeResult::Success
    })
}

fn calm() -> impl BehaviorNode<Guard> {
    advance(1)
}

fn angry() -> impl BehaviorNode<Guard> {
    advance(10)
}

#[test]
fn builders_sharing_a_tree_type_stay_separate() {
    let mut app = App::new();
    app.add_plugins((
        BehaviorPlugin::for_tree(calm),
        BehaviorPlugin::for_tree(angry),
    ));
    let quiet = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(calm)))
        .id();
    let loud = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(angry)))
        .id();

    app.update();

    assert_eq!(app.world().get::<Guard>(quiet).unwrap().ammo, 1);
    assert_eq!(app.world().get::<Guard>(loud).unwrap().ammo, 10);
}

// --- the gather is the game's ------------------------------------------------

#[derive(Resource, Default)]
struct Alarm(bool);

/// Every tick: the cheap half, and last tick's decisions cleared.
fn gather(mut agents: Query<&mut Guard>, alarm: Res<Alarm>) {
    for mut guard in agents.iter_mut() {
        let guard = guard.bypass_change_detection();
        guard.alarm = alarm.0;
        guard.reload = false;
    }
}

#[test]
fn what_the_tree_sees_is_whatever_the_game_gathered() {
    let mut app = app(shoot);
    app.init_resource::<Alarm>()
        .add_systems(Update, gather.before(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 3,
                ..Guard::default()
            },
            Behavior::for_tree(shoot),
        ))
        .id();

    app.update();
    assert_eq!(
        app.world().get::<Guard>(agent).unwrap().fired,
        0,
        "no alarm"
    );

    app.world_mut().resource_mut::<Alarm>().0 = true;
    app.update();
    assert_eq!(app.world().get::<Guard>(agent).unwrap().fired, 1);
}

// --- pacing ------------------------------------------------------------------

#[test]
fn evaluate_every_spreads_a_population_across_the_period() {
    let period = Duration::from_millis(100);
    let delta = Duration::from_millis(10);
    let evaluated = (0..1_000u32)
        .filter(|index| {
            let entity = Entity::from_raw_u32(*index).unwrap();
            evaluate_every(period, Duration::from_millis(500), delta, entity) == EntryMode::Evaluate
        })
        .count();

    assert!(
        (60..140).contains(&evaluated),
        "about a tenth of 1000, got {evaluated}"
    );
}

#[test]
fn a_frame_longer_than_the_period_evaluates_once() {
    let mode = evaluate_every(
        Duration::from_millis(10),
        Duration::from_millis(500),
        Duration::from_millis(250),
        Entity::from_raw_u32(7).unwrap(),
    );
    assert_eq!(mode, EntryMode::Evaluate);
}
