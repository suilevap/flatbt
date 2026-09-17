//! What each [`Tick`] does to an agent, and why `Skip` is not something a tree
//! can do for itself.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;

#[derive(Component, Default, Debug)]
struct Agent {
    awake: bool,
    /// Counts how often the guard at the root was actually consulted.
    guard_ran: u32,
    /// Counts how often the body ran.
    body_ran: u32,
}

/// Runs forever, standing in for work a system outside the tree carries out.
struct LongAction;

impl BtNode<Agent> for LongAction {
    type State = ();

    fn update(&self, _: &mut (), agent: &mut Agent, _: (), _: EntryMode) -> NodeResult {
        agent.body_ran += 1;
        NodeResult::Running
    }
}

fn counting_gate() -> impl BehaviorNode<Agent> {
    seq((
        leaf(|agent: &mut Agent| {
            agent.guard_ran += 1;
            if agent.awake {
                NodeResult::Success
            } else {
                NodeResult::Failure
            }
        }),
        LongAction,
    ))
}

#[test]
fn a_root_guard_is_skipped_entirely_once_the_tree_is_suspended() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(counting_gate).tick_mode(|_| Tick::Resume));
    let agent = app
        .world_mut()
        .spawn((
            Agent {
                awake: true,
                ..Agent::default()
            },
            Behavior::for_tree(counting_gate),
        ))
        .id();

    // First tick enters as Evaluate: the guard runs and lets the body through.
    app.update();
    let after_first = app.world().get::<Agent>(agent).unwrap();
    assert_eq!((after_first.guard_ran, after_first.body_ran), (1, 1));

    // Now close the gate and keep ticking.
    app.world_mut().get_mut::<Agent>(agent).unwrap().awake = false;
    for _ in 0..5 {
        app.update();
    }

    let after = app.world().get::<Agent>(agent).unwrap();
    assert_eq!(
        after.guard_ran, 1,
        "the guard was never consulted again -- Resume re-enters the active \
         child directly, so a child above it is not looked at"
    );
    assert_eq!(
        after.body_ran, 6,
        "and the body kept running with the gate shut"
    );
}

/// Under `Evaluate` the guard does work, which is the other half of the cost:
/// stopping a tree that way means re-deciding the whole tree every tick.
#[test]
fn under_evaluate_the_guard_works_but_the_tree_is_entered_every_tick() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(counting_gate));
    let agent = app
        .world_mut()
        .spawn((Agent::default(), Behavior::for_tree(counting_gate)))
        .id();

    for _ in 0..5 {
        app.update();
    }

    let after = app.world().get::<Agent>(agent).unwrap();
    assert_eq!(after.guard_ran, 5, "consulted every tick");
    assert_eq!(after.body_ran, 0, "and it did hold the body back");
}

/// And `Evaluate` does not save it either, because `seq` continues its active
/// child rather than rescanning: a guard that is child zero of a sequence is
/// not consulted again while the sequence is suspended inside child one.
///
/// This is the whole case for a skip. Neither entry mode consults a root guard
/// once the tree is suspended below it, so "do not run this agent's tree" is
/// not expressible inside the tree at all.
#[test]
fn a_root_guard_under_a_sequence_is_not_rechecked_on_evaluate_either() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(counting_gate).tick_mode(|_| Tick::Evaluate));
    let agent = app
        .world_mut()
        .spawn((
            Agent {
                awake: true,
                ..Agent::default()
            },
            Behavior::for_tree(counting_gate),
        ))
        .id();

    app.update();
    app.world_mut().get_mut::<Agent>(agent).unwrap().awake = false;
    for _ in 0..4 {
        app.update();
    }

    let after = app.world().get::<Agent>(agent).unwrap();
    assert_eq!(
        after.guard_ran, 1,
        "`seq` continues its active child on Evaluate, so child zero is never \
         looked at again"
    );
    assert_eq!(after.body_ran, 5, "the body ran on with the gate shut");
}

/// A `select` does rescan on `Evaluate` -- but a candidate that *fails* leaves
/// the standing branch in place and carries on running it. So even the shape
/// that rechecks cannot stop a tree: a guard can only redirect it to a branch
/// that succeeds, never hold it still.
#[test]
fn a_select_rescans_but_a_failing_guard_still_cannot_stop_the_tree() {
    fn selected() -> impl BehaviorNode<Agent> {
        select((seq((check(|a: &Agent| a.awake), LongAction)),))
    }

    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(selected).tick_mode(|_| Tick::Evaluate));
    let agent = app
        .world_mut()
        .spawn((
            Agent {
                awake: true,
                ..Agent::default()
            },
            Behavior::for_tree(selected),
        ))
        .id();

    app.update();
    app.update();
    assert_eq!(app.world().get::<Agent>(agent).unwrap().body_ran, 2);

    // Shut the gate. The selector rescans, the candidate fails on the guard --
    // and a failed candidate preserves the branch that was already running,
    // which then runs.
    app.world_mut().get_mut::<Agent>(agent).unwrap().awake = false;
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().get::<Agent>(agent).unwrap().body_ran,
        5,
        "the body kept running: failing a candidate is not stopping a tree"
    );
}

/// Stopping every tree at once is a run condition, and it is exact -- but it is
/// all of them, for every agent, not a per-agent decision.
#[test]
fn a_run_condition_does_stop_the_tick_but_takes_the_whole_population() {
    #[derive(Resource, Default)]
    struct Paused(bool);

    let mut app = App::new();
    app.init_resource::<Paused>()
        .add_plugins(BehaviorPlugin::for_tree(counting_gate).tick_mode(|_| Tick::Resume))
        .configure_sets(
            Update,
            BehaviorSystems.run_if(|paused: Res<Paused>| !paused.0),
        );
    let agent = app
        .world_mut()
        .spawn((
            Agent {
                awake: true,
                ..Agent::default()
            },
            Behavior::for_tree(counting_gate),
        ))
        .id();

    app.update();
    assert_eq!(app.world().get::<Agent>(agent).unwrap().body_ran, 1);

    app.world_mut().resource_mut::<Paused>().0 = true;
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(
        app.world().get::<Agent>(agent).unwrap().body_ran,
        1,
        "nothing ticked at all"
    );

    // And the suspended invocation survived, which is the behaviour a per-agent
    // skip would have to match.
    app.world_mut().resource_mut::<Paused>().0 = false;
    app.update();
    assert_eq!(
        app.world().get::<Agent>(agent).unwrap().body_ran,
        2,
        "it continued where it left off"
    );
    assert_eq!(
        app.world().get::<Agent>(agent).unwrap().guard_ran,
        1,
        "without re-entering from the root"
    );
}

// --- and what `Skip` does instead --------------------------------------------

/// The thing no shape above could do: hold one agent still, per agent, without
/// touching its suspended invocation.
#[test]
fn skip_leaves_one_agent_untouched_while_the_rest_tick() {
    let mut app = App::new();
    app.add_plugins(
        BehaviorPlugin::for_tree(counting_gate).tick_mode(|agent: &Agent| {
            if agent.awake {
                Tick::Resume
            } else {
                Tick::Skip
            }
        }),
    );
    let sleeper = app
        .world_mut()
        .spawn((
            Agent {
                awake: true,
                ..Agent::default()
            },
            Behavior::for_tree(counting_gate),
        ))
        .id();
    let worker = app
        .world_mut()
        .spawn((
            Agent {
                awake: true,
                ..Agent::default()
            },
            Behavior::for_tree(counting_gate),
        ))
        .id();

    // Both enter and suspend inside the body.
    app.update();
    assert_eq!(app.world().get::<Agent>(sleeper).unwrap().body_ran, 1);
    assert_eq!(app.world().get::<Agent>(worker).unwrap().body_ran, 1);

    // Put one to sleep. It stops; its neighbour does not.
    app.world_mut().get_mut::<Agent>(sleeper).unwrap().awake = false;
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(
        app.world().get::<Agent>(sleeper).unwrap().body_ran,
        1,
        "skipped entirely, which no guard inside the tree could manage"
    );
    assert_eq!(app.world().get::<Agent>(worker).unwrap().body_ran, 6);

    // Wake it: it resumes into the node it was in, not a fresh invocation.
    app.world_mut().get_mut::<Agent>(sleeper).unwrap().awake = true;
    app.update();
    let after = app.world().get::<Agent>(sleeper).unwrap();
    assert_eq!(after.body_ran, 2, "continued where it was");
    assert_eq!(
        after.guard_ran, 1,
        "without re-entering the tree from the root"
    );
}

/// A skipped agent that was never entered stays that way, so `Skip` cannot be
/// used to start something.
#[test]
fn skip_before_the_first_tick_leaves_the_agent_with_no_invocation() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(counting_gate).tick_mode(|_| Tick::Skip));
    let agent = app
        .world_mut()
        .spawn((
            Agent {
                awake: true,
                ..Agent::default()
            },
            Behavior::for_tree(counting_gate),
        ))
        .id();

    for _ in 0..3 {
        app.update();
    }

    let after = app.world().get::<Agent>(agent).unwrap();
    assert_eq!((after.guard_ran, after.body_ran), (0, 0));
}

#[test]
fn tick_converts_from_an_entry_mode() {
    assert_eq!(Tick::from(EntryMode::Resume), Tick::Resume);
    assert_eq!(Tick::from(EntryMode::Evaluate), Tick::Evaluate);
    assert_eq!(Tick::Skip.entry_mode(), None);
    assert_eq!(Tick::Resume.entry_mode(), Some(EntryMode::Resume));
}
