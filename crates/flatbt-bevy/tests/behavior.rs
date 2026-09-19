//! Ticking agents: what the plugin registers, and what a tree's decision
//! becomes once it leaves the tree.

use core::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;

/// What a guard knows. The tree reads it; the gather writes it.
#[derive(Component, Default, Debug)]
struct Guard {
    ammo: u32,
    alarm: bool,
}

/// What a guard is doing. An order to the world, not a change to it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum Act {
    Firing,
    Loading,
}

/// Firing keeps its own condition, which is what ends it.
///
/// A guard above the leaf would not do: once the tree is suspended in the leaf,
/// no entry mode consults a child above it, so the act would stand after the
/// magazine ran out. Whatever keeps an agent busy is what decides when to stop
/// -- which is `is_in_progress` on a real action.
fn shoot() -> impl BehaviorNode<Guard, Act> {
    seq((leaf(|guard: &mut Guard| {
        if guard.alarm && guard.ammo > 0 {
            NodeResult::Running(Act::Firing)
        } else {
            NodeResult::Failure
        }
    }),))
}

fn app<F: TreeBuilder<Guard, Act>>(builder: F) -> App {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(builder));
    app
}

#[test]
fn a_running_tree_puts_its_act_on_the_agent() {
    let mut app = app(shoot);
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 2,
                alarm: true,
            },
            Behavior::for_tree(shoot),
        ))
        .id();

    app.update();

    assert_eq!(app.world().get::<Act>(agent), Some(&Act::Firing));
}

#[test]
fn an_agent_whose_tree_fails_is_doing_nothing() {
    let mut app = app(shoot);
    let agent = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(shoot)))
        .id();

    app.update();

    assert_eq!(
        app.world().get::<Act>(agent),
        None,
        "no alarm, so the tree failed and the agent carries no act"
    );
}

/// The act component comes and goes with the decision, so a query over it is
/// exactly the agents with a standing order.
#[test]
fn the_act_goes_when_the_decision_does() {
    let mut app = app(shoot);
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 1,
                alarm: true,
            },
            Behavior::for_tree(shoot),
        ))
        .id();

    app.update();
    assert_eq!(app.world().get::<Act>(agent), Some(&Act::Firing));

    // The world spent the round, so the guard has nothing to fire with.
    app.world_mut().get_mut::<Guard>(agent).unwrap().ammo = 0;
    app.update();
    assert_eq!(app.world().get::<Act>(agent), None);
}

#[test]
fn an_agent_without_the_blackboard_is_skipped() {
    let mut app = app(shoot);
    let bare = app.world_mut().spawn(Behavior::for_tree(shoot)).id();

    app.update();

    assert!(app.world().get::<Guard>(bare).is_none());
    assert!(app.world().get::<Act>(bare).is_none());
}

// --- an act that changes without the component moving ------------------------

#[derive(Component, Default, Debug)]
struct Walker {
    target: f32,
    position: f32,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct WalkingTo(f32);

/// Restates where it is going on every update, without ending.
struct Approach;

impl BtNode<Walker, WalkingTo> for Approach {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        walker: &mut Walker,
        _: (),
        _: EntryMode,
    ) -> NodeResult<WalkingTo> {
        if walker.position == walker.target {
            return NodeResult::Success;
        }
        NodeResult::Running(WalkingTo(walker.target))
    }
}

fn approach() -> impl BehaviorNode<Walker, WalkingTo> {
    seq((Approach,))
}

/// A standing order that only changes value is written in place: the agent
/// never moves archetype, which is what makes this affordable at scale.
#[test]
fn an_act_that_only_changes_is_written_in_place() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(approach).tick_mode(|_| Tick::Resume));
    let agent = app
        .world_mut()
        .spawn((
            Walker {
                target: 10.0,
                position: 0.0,
            },
            Behavior::for_tree(approach),
        ))
        .id();

    app.update();
    assert_eq!(app.world().get::<WalkingTo>(agent), Some(&WalkingTo(10.0)));
    let after_first = app.world().entity(agent).archetype().id();

    // The target moves; the order follows it.
    app.world_mut().get_mut::<Walker>(agent).unwrap().target = 25.0;
    app.update();
    assert_eq!(app.world().get::<WalkingTo>(agent), Some(&WalkingTo(25.0)));
    assert_eq!(
        app.world().entity(agent).archetype().id(),
        after_first,
        "the act changed without the entity moving archetype"
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
                    },
                    Behavior::for_tree(shoot),
                ))
                .id()
        })
        .collect();

    app.update();

    for (index, agent) in agents.iter().enumerate() {
        let expected = if (index as u32).is_multiple_of(4) {
            None
        } else {
            Some(&Act::Firing)
        };
        assert_eq!(
            app.world().get::<Act>(*agent),
            expected,
            "agent {index} decided for itself"
        );
    }
}

#[test]
fn only_the_invocation_state_lives_on_the_agent() {
    assert_eq!(
        core::mem::size_of::<Behavior<Walker, WalkingTo, fn() -> Approach>>(),
        core::mem::size_of::<Option<()>>(),
        "no tree, no builder value, nothing but the state"
    );
}

// --- two trees over one blackboard -------------------------------------------

fn hoard() -> impl BehaviorNode<Guard, Act> {
    leaf(|_: &mut Guard| NodeResult::Running(Act::Loading))
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
            },
            Behavior::for_tree(shoot),
        ))
        .id();
    let hoarder = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(hoard)))
        .id();

    app.update();

    assert_eq!(app.world().get::<Act>(shooter), Some(&Act::Firing));
    assert_eq!(app.world().get::<Act>(hoarder), Some(&Act::Loading));
}

/// Two names for one tree type, which is why the builder is the identity.
#[test]
fn builders_sharing_a_tree_type_stay_separate() {
    fn armed(rounds: u32) -> impl BehaviorNode<Guard, Act> {
        seq((
            check(move |guard: &Guard| guard.ammo >= rounds),
            leaf(|_: &mut Guard| NodeResult::Running(Act::Firing)),
        ))
    }

    fn careful() -> impl BehaviorNode<Guard, Act> {
        armed(3)
    }

    fn reckless() -> impl BehaviorNode<Guard, Act> {
        armed(1)
    }

    let mut app = App::new();
    app.add_plugins((
        BehaviorPlugin::for_tree(careful),
        BehaviorPlugin::for_tree(reckless),
    ));
    let cautious = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 2,
                alarm: false,
            },
            Behavior::for_tree(careful),
        ))
        .id();
    let eager = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 2,
                alarm: false,
            },
            Behavior::for_tree(reckless),
        ))
        .id();

    app.update();

    assert_eq!(
        app.world().get::<Act>(cautious),
        None,
        "two rounds is not enough"
    );
    assert_eq!(app.world().get::<Act>(eager), Some(&Act::Firing));
}

// --- the gather decides what the tree sees -----------------------------------

#[test]
fn what_the_tree_sees_is_whatever_the_game_gathered() {
    fn raise_alarm(mut guards: Query<&mut Guard>) {
        for mut guard in guards.iter_mut() {
            guard.bypass_change_detection().alarm = true;
        }
    }

    let mut app = app(shoot);
    app.add_systems(Update, raise_alarm.before(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 1,
                alarm: false,
            },
            Behavior::for_tree(shoot),
        ))
        .id();

    app.update();

    assert_eq!(
        app.world().get::<Act>(agent),
        Some(&Act::Firing),
        "the gather raised the alarm before the tick read it"
    );
}

// --- pacing ------------------------------------------------------------------

#[test]
fn evaluate_every_spreads_a_population_across_the_period() {
    let period = Duration::from_millis(100);
    let delta = Duration::from_millis(10);
    let evaluated = (0..1_000u32)
        .filter(|index| {
            let entity = Entity::from_raw_u32(*index).unwrap();
            evaluate_every(period, Duration::from_millis(500), delta, entity) == Tick::Evaluate
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
    assert_eq!(mode, Tick::Evaluate);
}

/// The two policies differ only in what they answer between slots, which is the
/// whole choice: resume ticks an action along, skip does not.
#[test]
fn act_every_skips_between_slots_where_evaluate_every_resumes() {
    let period = Duration::from_millis(100);
    let delta = Duration::from_millis(10);
    let clock = Duration::from_millis(500);
    let mut between = 0;
    for index in 0..1_000u32 {
        let entity = Entity::from_raw_u32(index).unwrap();
        match (
            evaluate_every(period, clock, delta, entity),
            act_every(period, clock, delta, entity),
        ) {
            (Tick::Evaluate, Tick::Evaluate) => {}
            (Tick::Resume, Tick::Skip) => between += 1,
            other => panic!("the two policies disagreed on a slot: {other:?}"),
        }
    }
    assert!(between > 0);
}
