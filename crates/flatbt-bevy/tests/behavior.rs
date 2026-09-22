//! Ticking agents: what the plugin registers, and what a tree's decision
//! becomes once it leaves the tree.

use core::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use bevy_time::{TimePlugin, TimeUpdateStrategy};
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

// --- starting, stopping, changing --------------------------------------------

fn firing_agent(app: &mut App) -> Entity {
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 9,
                alarm: true,
            },
            Behavior::for_tree(shoot),
        ))
        .id();
    app.update();
    assert_eq!(app.world().get::<Act>(agent), Some(&Act::Firing));
    agent
}

/// An act outlives the tick that wrote it -- the systems carrying it out run
/// afterwards -- so stopping an agent has to take it back. The tick cannot:
/// once the `Behavior` is gone the agent is not in its query any more.
#[test]
fn stopping_an_agent_takes_back_its_act() {
    let mut app = app(shoot);
    let agent = firing_agent(&mut app);

    app.world_mut().entity_mut(agent).stop_behavior(shoot);

    assert_eq!(
        app.world().get::<Act>(agent),
        None,
        "the order went with the component that was giving it"
    );
}

/// And it goes there and then, not on some next tick -- which may never come,
/// in a schedule the game runs itself.
#[test]
fn the_act_goes_without_waiting_for_another_tick() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(shoot).in_schedule(TurnPhase));
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 9,
                alarm: true,
            },
            Behavior::for_tree(shoot),
        ))
        .id();
    app.world_mut().run_schedule(TurnPhase);
    assert_eq!(app.world().get::<Act>(agent), Some(&Act::Firing));

    app.world_mut().entity_mut(agent).stop_behavior(shoot);

    assert_eq!(app.world().get::<Act>(agent), None);
}

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TurnPhase;

/// Stopping from a system is the same thing through `Commands`.
#[test]
fn an_agent_can_be_stopped_from_a_system() {
    fn stand_down(mut commands: Commands, agents: Query<Entity, With<Act>>) {
        for agent in agents.iter() {
            commands.entity(agent).stop_behavior(shoot);
        }
    }

    let mut app = app(shoot);
    let agent = firing_agent(&mut app);
    app.add_systems(Update, stand_down.after(BehaviorSystems));

    app.update();

    assert_eq!(app.world().get::<Act>(agent), None);
    assert!(
        app.world().get::<Guard>(agent).is_some(),
        "the agent is still there -- it is just not running a tree"
    );
}

/// Swapping one tree for another goes through the same door: the old act is
/// released as the old `Behavior` goes, and the new tree decides from scratch.
#[test]
fn changing_an_agent_to_another_tree_replaces_its_act() {
    let mut app = App::new();
    app.add_plugins((
        BehaviorPlugin::for_tree(shoot),
        BehaviorPlugin::for_tree(hoard),
    ));
    let agent = firing_agent(&mut app);

    app.world_mut()
        .entity_mut(agent)
        .stop_behavior(shoot)
        .insert(Behavior::for_tree(hoard));
    app.update();

    assert_eq!(app.world().get::<Act>(agent), Some(&Act::Loading));
}

#[test]
fn despawning_an_agent_that_was_doing_something_is_quiet() {
    let mut app = app(shoot);
    let agent = firing_agent(&mut app);

    app.world_mut().entity_mut(agent).despawn();
    app.update();

    assert!(app.world().get_entity(agent).is_err());
}

// --- an agent is a behavior *and* a blackboard --------------------------------

/// Records the mode it was entered with, which is how these tests see whether
/// an invocation was resumed or started again.
#[derive(Component, Default, Debug)]
struct Trace(Vec<EntryMode>);

struct Remember;

impl BtNode<Trace, Act> for Remember {
    type State = ();

    fn update(&self, _: &mut (), trace: &mut Trace, _: (), mode: EntryMode) -> NodeResult<Act> {
        trace.0.push(mode);
        NodeResult::Running(Act::Firing)
    }
}

fn remembering() -> impl BehaviorNode<Trace, Act> {
    seq((Remember,))
}

fn traced_app() -> App {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(remembering).tick_mode(|_, _| Tick::Resume));
    app
}

fn modes(app: &App, agent: Entity) -> Vec<EntryMode> {
    app.world().get::<Trace>(agent).unwrap().0.clone()
}

/// `restart` forgets where the invocation was without stopping the agent.
#[test]
fn restarting_an_agent_enters_from_the_root() {
    let mut app = traced_app();
    let agent = app
        .world_mut()
        .spawn((Trace::default(), Behavior::for_tree(remembering)))
        .id();
    app.update();
    app.update();
    assert_eq!(modes(&app, agent), [EntryMode::Evaluate, EntryMode::Resume]);

    app.world_mut()
        .entity_mut(agent)
        .restart_behavior(remembering);
    app.update();

    assert_eq!(
        modes(&app, agent),
        [EntryMode::Evaluate, EntryMode::Resume, EntryMode::Evaluate],
        "still an agent, but with nothing to resume into"
    );
    assert_eq!(app.world().get::<Act>(agent), Some(&Act::Firing));
}

// --- what the blackboard is, and is not --------------------------------------

#[derive(Resource, Default)]
struct Noticed(u32);

/// The tick passes the blackboard with change detection bypassed, so a node
/// writing to it -- which is how nodes leave notes for each other -- does not
/// mark it changed. A pinned compromise, not an accident: the gather rewrites
/// the blackboard every tick anyway, and marking a whole population changed
/// every frame would drag the rest of the engine along. The tree's output is
/// the act; the blackboard is its input.
#[test]
fn a_node_writing_to_the_blackboard_does_not_mark_it_changed() {
    fn tally() -> impl BehaviorNode<Guard, Act> {
        leaf(|guard: &mut Guard| {
            guard.ammo += 1;
            NodeResult::Running(Act::Loading)
        })
    }

    fn notice(changed: Query<(), Changed<Guard>>, mut noticed: ResMut<Noticed>) {
        noticed.0 += changed.iter().count() as u32;
    }

    let mut app = app(tally);
    app.init_resource::<Noticed>()
        .add_systems(Update, notice.after(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(tally)))
        .id();

    // The first frame sees the blackboard as changed because it was just added.
    app.update();
    app.world_mut().resource_mut::<Noticed>().0 = 0;
    app.update();

    assert_eq!(
        app.world().get::<Guard>(agent).unwrap().ammo,
        2,
        "the write itself lands, and the next tick reads it back"
    );
    assert_eq!(
        app.world().resource::<Noticed>().0,
        0,
        "but no `Changed<Guard>` filter saw it"
    );
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
    app.add_plugins(BehaviorPlugin::for_tree(approach).tick_mode(|_, _| Tick::Resume));
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
        leaf(move |guard: &mut Guard| {
            if guard.ammo >= rounds {
                NodeResult::Running(Act::Firing)
            } else {
                NodeResult::Failure
            }
        })
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
            guard.alarm = true;
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
            let at = TickAt {
                entity: Entity::from_raw_u32(*index).unwrap(),
                elapsed: Duration::from_millis(500),
                delta,
            };
            evaluate_every(period, at) == Tick::Evaluate
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
        TickAt {
            entity: Entity::from_raw_u32(7).unwrap(),
            elapsed: Duration::from_millis(500),
            delta: Duration::from_millis(250),
        },
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
        let at = TickAt {
            entity: Entity::from_raw_u32(index).unwrap(),
            elapsed: clock,
            delta,
        };
        match (evaluate_every(period, at), act_every(period, at)) {
            (Tick::Evaluate, Tick::Evaluate) => {}
            (Tick::Resume, Tick::Skip) => between += 1,
            other => panic!("the two policies disagreed on a slot: {other:?}"),
        }
    }
    assert!(between > 0);
}

/// How often a tree was entered.
#[derive(Component, Default)]
struct Entries(u32);

fn counted() -> impl BehaviorNode<Entries, Act> {
    leaf(|entries: &mut Entries| {
        entries.0 += 1;
        NodeResult::Running(Act::Firing)
    })
}

/// The tick hands `tick_mode` the agent and the schedule's clock, so a policy
/// that needs both is written in place.
#[test]
fn act_every_in_tick_mode_enters_each_agent_about_once_per_period() {
    let mut app = App::new();
    app.add_plugins(TimePlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            10,
        )))
        .add_plugins(
            BehaviorPlugin::for_tree(counted)
                .tick_mode(|_, at| act_every(Duration::from_millis(100), at)),
        );
    let agents: Vec<Entity> = (0..20)
        .map(|_| {
            app.world_mut()
                .spawn((Entries::default(), Behavior::for_tree(counted)))
                .id()
        })
        .collect();
    // One frame to start the clock, then three periods.
    for _ in 0..31 {
        app.update();
    }
    for agent in agents {
        let entries = app.world().get::<Entries>(agent).unwrap().0;
        assert!((2..=4).contains(&entries), "entered {entries} times");
    }
}
