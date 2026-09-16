use core::time::Duration;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use flatbt_bevy::prelude::*;

#[derive(Component, Debug, PartialEq)]
struct Ammo(u32);

#[derive(Component, Debug, PartialEq)]
struct Fired(u32);

#[derive(Component)]
struct Reloading;

#[derive(Resource)]
struct Alarm(bool);

/// What the guard trees see: plain data, gathered once per tick.
struct Guard {
    ammo: u32,
    fired: u32,
    alarm: bool,
    alarm_changed: bool,
}

#[derive(QueryData)]
#[query_data(mutable)]
struct GuardAccess {
    ammo: &'static mut Ammo,
    fired: &'static mut Fired,
}

impl BehaviorContext for Guard {
    type Agent = GuardAccess;
    type Param = Res<'static, Alarm>;
    type Snapshot = Self;

    fn read(_: Entity, agent: &GuardAccessItem, alarm: &Res<Alarm>) -> Guard {
        Guard {
            ammo: agent.ammo.0,
            fired: agent.fired.0,
            alarm: alarm.0,
            alarm_changed: alarm.is_changed(),
        }
    }

    fn write(guard: &Guard, agent: &mut GuardAccessItem) {
        agent.ammo.set_if_neq(Ammo(guard.ammo));
        agent.fired.set_if_neq(Fired(guard.fired));
    }

    /// Standing decisions hold until the alarm itself changes.
    fn entry_mode(bb: &Blackboard<Guard>) -> EntryMode {
        if bb.alarm_changed {
            EntryMode::Evaluate
        } else {
            EntryMode::Resume
        }
    }
}

fn shoot() -> impl BehaviorNode<Guard> {
    seq((
        check(|bb: &Blackboard<Guard>| bb.alarm),
        check(|bb: &Blackboard<Guard>| bb.ammo > 0),
        leaf(|bb: &mut Blackboard<Guard>| {
            bb.ammo -= 1;
            bb.fired += 1;
            NodeResult::Success
        }),
    ))
}

/// One registration per tree; both type parameters come from the builder.
fn app<F: TreeBuilder<Guard>>(builder: F) -> App {
    let mut app = App::new();
    app.insert_resource(Alarm(true))
        .add_plugins(BehaviorPlugin::for_tree(builder));
    app
}

#[test]
fn ticks_agent_components_until_the_guard_fails() {
    let mut app = app(shoot);
    let entity = app
        .world_mut()
        .spawn((Ammo(2), Fired(0), Behavior::for_tree(shoot)))
        .id();

    for _ in 0..4 {
        app.update();
    }

    assert_eq!(app.world().get::<Ammo>(entity), Some(&Ammo(0)));
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(2)));
}

#[test]
fn shared_access_gates_the_tree() {
    let mut app = app(shoot);
    app.insert_resource(Alarm(false));
    let entity = app
        .world_mut()
        .spawn((Ammo(2), Fired(0), Behavior::for_tree(shoot)))
        .id();

    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));

    app.world_mut().resource_mut::<Alarm>().0 = true;
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(1)));
}

#[test]
fn one_tree_serves_many_agents_in_parallel() {
    let mut app = App::new();
    app.insert_resource(Alarm(true))
        .add_plugins(BehaviorPlugin::for_tree(shoot).parallel());

    let agents: Vec<Entity> = {
        let world = app.world_mut();
        (0..64)
            .map(|_| {
                world
                    .spawn((Ammo(1), Fired(0), Behavior::for_tree(shoot)))
                    .id()
            })
            .collect()
    };

    app.update();

    for agent in agents {
        assert_eq!(app.world().get::<Fired>(agent), Some(&Fired(1)));
    }
}

fn reload_when_dry() -> impl BehaviorNode<Guard> {
    select((
        shoot(),
        leaf(|bb: &mut Blackboard<Guard>| {
            bb.agent_commands().insert(Reloading);
            NodeResult::Success
        }),
    ))
}

#[test]
fn nodes_defer_world_edits_through_commands() {
    let mut app = app(reload_when_dry);
    let entity = app
        .world_mut()
        .spawn((Ammo(0), Fired(0), Behavior::for_tree(reload_when_dry)))
        .id();

    app.update();

    assert!(app.world().get::<Reloading>(entity).is_some());
}

/// Suspends for a fixed number of ticks, holding state between updates.
/// A named node type, so `Behavior<Guard, Recharge>` can be written out.
struct Recharge(u32);

impl<C: BehaviorContext> BtNode<Blackboard<C>> for Recharge {
    type State = u32;

    fn update(&self, elapsed: &mut u32, _: &mut Blackboard<C>, _: (), _: EntryMode) -> NodeResult {
        *elapsed += 1;
        if *elapsed >= self.0 {
            NodeResult::Success
        } else {
            NodeResult::Running
        }
    }
}

fn recharge_then_shoot() -> impl BehaviorNode<Guard> {
    seq((Recharge(3), shoot()))
}

#[test]
fn running_state_survives_across_ticks() {
    let mut app = app(recharge_then_shoot);
    let entity = app
        .world_mut()
        .spawn((Ammo(1), Fired(0), Behavior::for_tree(recharge_then_shoot)))
        .id();

    for _ in 0..2 {
        app.update();
        assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));
    }

    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(1)));
}

/// A node with bulky invocation state.
struct Bulky;

struct Trail(#[allow(dead_code)] [u32; 64]);

impl Default for Trail {
    fn default() -> Self {
        Self([0; 64])
    }
}

impl<C: BehaviorContext> BtNode<Blackboard<C>> for Bulky {
    type State = Trail;

    fn update(&self, _: &mut Trail, _: &mut Blackboard<C>, _: (), _: EntryMode) -> NodeResult {
        NodeResult::Success
    }
}

#[test]
fn only_agent_state_lives_in_the_component() {
    // No box, no tree copy, no flags: a plain function builder is zero-sized,
    // so the component is its invocation state and the entry mode.
    assert!(size_of_val(&Behavior::<Guard, _>::for_tree(recharge)) <= 16);
    assert!(size_of_val(&Behavior::<Guard, _>::for_tree(bulky)) >= size_of::<Trail>());
}

/// Suspends in the fallback branch, so a selector that rescans on Evaluate
/// visibly differs from one that resumes.
fn hold_or_fire() -> impl BehaviorNode<Guard> {
    select((
        seq((
            check(|bb: &Blackboard<Guard>| bb.alarm),
            leaf(|bb: &mut Blackboard<Guard>| {
                bb.fired += 1;
                NodeResult::Success
            }),
        )),
        Recharge(10),
    ))
}

/// A second name for the same tree, so two registrations can differ in pace.
fn hold_or_fire_too() -> impl BehaviorNode<Guard> {
    hold_or_fire()
}

#[test]
fn the_context_decides_when_a_standing_decision_is_stale() {
    let mut app = app(hold_or_fire);
    app.insert_resource(Alarm(false));
    let entity = app
        .world_mut()
        .spawn((Ammo(1), Fired(0), Behavior::for_tree(hold_or_fire)))
        .id();

    // No alarm: the tree settles into the fallback and suspends there.
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));

    // Nothing changed, so the branch already chosen simply resumes.
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));

    // The alarm moves: the context asks for Evaluate and the selector rescans.
    app.world_mut().resource_mut::<Alarm>().0 = true;
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(1)));
}

#[derive(Resource)]
struct Halted(bool);

#[test]
fn a_run_condition_on_the_set_gates_self_registered_ticks() {
    let mut app = app(shoot);
    app.insert_resource(Halted(false)).configure_sets(
        Update,
        BehaviorSystems.run_if(|halted: Res<Halted>| !halted.0),
    );
    let entity = app
        .world_mut()
        .spawn((Ammo(4), Fired(0), Behavior::for_tree(shoot)))
        .id();

    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(1)));

    // The tick system is added at runtime, and still inherits the set.
    app.world_mut().resource_mut::<Halted>().0 = true;
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(1)));

    app.world_mut().resource_mut::<Halted>().0 = false;
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(2)));
}

fn advance(step: u32) -> impl BehaviorNode<Guard> {
    leaf(move |bb: &mut Blackboard<Guard>| {
        bb.fired += step;
        NodeResult::Success
    })
}

// Two builders returning the same tree type with different node configuration.
fn calm() -> impl BehaviorNode<Guard> {
    advance(1)
}

fn angry() -> impl BehaviorNode<Guard> {
    advance(10)
}

#[test]
fn builders_sharing_a_tree_type_stay_separate() {
    let mut app = App::new();
    app.insert_resource(Alarm(true)).add_plugins((
        BehaviorPlugin::for_tree(calm),
        BehaviorPlugin::for_tree(angry),
    ));

    let quiet = app
        .world_mut()
        .spawn((Ammo(0), Fired(0), Behavior::for_tree(calm)))
        .id();
    let loud = app
        .world_mut()
        .spawn((Ammo(0), Fired(0), Behavior::for_tree(angry)))
        .id();

    app.update();

    assert_eq!(app.world().get::<Fired>(quiet), Some(&Fired(1)));
    assert_eq!(app.world().get::<Fired>(loud), Some(&Fired(10)));
}

#[test]
fn entities_missing_agent_components_are_skipped() {
    let mut app = app(shoot);
    let partial = app
        .world_mut()
        .spawn((Ammo(1), Behavior::for_tree(shoot)))
        .id();
    let complete = app
        .world_mut()
        .spawn((Ammo(1), Fired(0), Behavior::for_tree(shoot)))
        .id();

    app.update();

    assert_eq!(app.world().get::<Ammo>(partial), Some(&Ammo(1)));
    assert_eq!(app.world().get::<Fired>(complete), Some(&Fired(1)));
}

fn bulky() -> Bulky {
    Bulky
}

fn recharge() -> Recharge {
    Recharge(1)
}

#[test]
fn two_trees_over_one_context_each_get_their_own_tick() {
    let mut app = App::new();
    app.insert_resource(Alarm(true)).add_plugins((
        BehaviorPlugin::for_tree(shoot),
        BehaviorPlugin::for_tree(reload_when_dry),
    ));

    let shooter = app
        .world_mut()
        .spawn((Ammo(2), Fired(0), Behavior::for_tree(shoot)))
        .id();
    let reloader = app
        .world_mut()
        .spawn((Ammo(0), Fired(0), Behavior::for_tree(reload_when_dry)))
        .id();

    app.update();

    assert_eq!(app.world().get::<Fired>(shooter), Some(&Fired(1)));
    assert!(app.world().get::<Reloading>(reloader).is_some());
}

#[test]
fn an_agent_whose_tree_was_never_registered_is_reported() {
    // No registration: no tick system queries this component type, so the agent
    // would sit there silently. The hook is the only thing that can say so.
    let mut app = App::new();
    app.insert_resource(Alarm(true));
    let entity = app
        .world_mut()
        .spawn((Ammo(1), Fired(0), Behavior::for_tree(shoot)))
        .id();
    app.update();

    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));
}

// --- periodic revalidation ---------------------------------------------------

/// Ticks `count` frames of `delta` and reports, per frame, how many of
/// `entities` were told to evaluate.
fn evaluations_per_frame(
    period: Duration,
    delta: Duration,
    frames: u32,
    entities: &[Entity],
) -> Vec<usize> {
    (1..=frames)
        .map(|frame| {
            let elapsed = delta * frame;
            entities
                .iter()
                .filter(|&&entity| {
                    evaluate_every(period, elapsed, delta, entity) == EntryMode::Evaluate
                })
                .count()
        })
        .collect()
}

fn agents(count: u32) -> Vec<Entity> {
    (0..count).filter_map(Entity::from_raw_u32).collect()
}

#[test]
fn each_agent_evaluates_once_per_period() {
    let period = Duration::from_millis(200);
    let delta = Duration::from_millis(16);
    // 10 periods at 16ms is 125 frames.
    let per_agent: Vec<usize> = agents(64)
        .iter()
        .map(|&entity| evaluations_per_frame(period, delta, 125, &[entity]))
        .map(|frames| frames.iter().sum())
        .collect();

    assert!(
        per_agent.iter().all(|&n| (9..=11).contains(&n)),
        "expected ~10 evaluations per agent over 10 periods, got {per_agent:?}",
    );
}

#[test]
fn a_population_does_not_evaluate_on_one_frame() {
    let period = Duration::from_millis(200);
    let delta = Duration::from_millis(16);
    let entities = agents(64);
    let busiest = evaluations_per_frame(period, delta, 125, &entities)
        .into_iter()
        .max()
        .unwrap();

    // Perfectly even would be 64 / 12.5 ≈ 5 per frame; aligned would be 64.
    assert!(busiest <= 16, "one frame carried {busiest} of 64 agents");
}

#[test]
fn a_frame_longer_than_the_period_evaluates_once() {
    let entity = Entity::from_raw_u32(7).unwrap();
    let period = Duration::from_millis(200);
    let delta = Duration::from_secs(1);
    assert_eq!(
        evaluate_every(period, delta, delta, entity),
        EntryMode::Evaluate
    );
}

// --- turn based --------------------------------------------------------------

/// Whose turn it is. Only the holder matches the agent query, so only the
/// holder is ticked: the query is the gate, and the game moves the marker.
#[derive(Component)]
struct Turn;

#[derive(Component, Debug, Default, PartialEq)]
struct Journal(Vec<u32>);

#[derive(Resource, Default)]
struct Clock {
    tick: u32,
}

struct Fighter {
    journal: Vec<u32>,
    tick: u32,
}

#[derive(QueryData)]
#[query_data(mutable)]
struct FighterAccess {
    _turn: &'static Turn,
    journal: &'static mut Journal,
}

impl BehaviorContext for Fighter {
    type Agent = FighterAccess;
    type Param = Res<'static, Clock>;
    type Snapshot = Self;

    fn read(_: Entity, agent: &FighterAccessItem, clock: &Res<Clock>) -> Fighter {
        Fighter {
            journal: agent.journal.0.clone(),
            tick: clock.tick,
        }
    }

    fn write(fighter: &Fighter, agent: &mut FighterAccessItem) {
        if agent.journal.0 != fighter.journal {
            agent.journal.0.clone_from(&fighter.journal);
        }
    }
}

/// A turn that spans more than one tick: act, wait, act, then hand the turn on.
fn take_turn() -> impl BehaviorNode<Fighter> {
    seq((
        leaf(|bb: &mut Blackboard<Fighter>| {
            let tick = bb.tick;
            bb.journal.push(tick);
            NodeResult::Success
        }),
        Recharge(2),
        leaf(|bb: &mut Blackboard<Fighter>| {
            let tick = bb.tick;
            bb.journal.push(tick);
            bb.agent_commands().remove::<Turn>();
            NodeResult::Success
        }),
    ))
}

fn advance_clock(mut clock: ResMut<Clock>) {
    clock.tick += 1;
}

/// The game owns the order: when nobody holds the turn, the next fighter gets it.
fn pass_the_turn(
    holders: Query<(), With<Turn>>,
    order: Res<Order>,
    mut next: Local<usize>,
    mut commands: Commands,
) {
    if holders.is_empty() {
        *next = (*next + 1) % order.0.len();
        commands.entity(order.0[*next]).insert(Turn);
    }
}

#[derive(Resource)]
struct Order(Vec<Entity>);

#[test]
fn agents_can_be_ticked_one_at_a_time_in_an_order_the_game_sets() {
    let mut app = App::new();
    app.init_resource::<Clock>()
        .add_plugins(BehaviorPlugin::for_tree(take_turn))
        .add_systems(Update, advance_clock.before(BehaviorSystems))
        .add_systems(Update, pass_the_turn.after(BehaviorSystems));

    let fighters: Vec<Entity> = (0..2)
        .map(|_| {
            app.world_mut()
                .spawn((Journal::default(), Behavior::for_tree(take_turn)))
                .id()
        })
        .collect();
    app.insert_resource(Order(fighters.clone()));
    app.world_mut().entity_mut(fighters[0]).insert(Turn);

    for _ in 0..6 {
        app.update();
    }

    // Each turn spans two ticks and resumes where it suspended; no fighter acts
    // inside another's turn; and the order comes back round.
    assert_eq!(
        app.world().get::<Journal>(fighters[0]),
        Some(&Journal(vec![1, 2, 5, 6]))
    );
    assert_eq!(
        app.world().get::<Journal>(fighters[1]),
        Some(&Journal(vec![3, 4]))
    );
}

// --- per-tree revalidation policy --------------------------------------------

fn always_evaluate(_: &Blackboard<Guard>) -> EntryMode {
    EntryMode::Evaluate
}

fn never_evaluate(_: &Blackboard<Guard>) -> EntryMode {
    EntryMode::Resume
}

#[test]
fn trees_sharing_a_context_can_pace_revalidation_differently() {
    let mut app = App::new();
    app.insert_resource(Alarm(false)).add_plugins((
        BehaviorPlugin::for_tree(hold_or_fire).entry_mode(always_evaluate),
        BehaviorPlugin::for_tree(hold_or_fire_too).entry_mode(never_evaluate),
    ));

    let eager = app
        .world_mut()
        .spawn((Ammo(1), Fired(0), Behavior::for_tree(hold_or_fire)))
        .id();
    let patient = app
        .world_mut()
        .spawn((Ammo(1), Fired(0), Behavior::for_tree(hold_or_fire_too)))
        .id();

    // Both settle into the fallback and suspend there.
    app.update();
    app.world_mut().resource_mut::<Alarm>().0 = true;
    app.update();

    // Same context, same access, same tree shape: only the pace differs.
    assert_eq!(app.world().get::<Fired>(eager), Some(&Fired(1)));
    assert_eq!(app.world().get::<Fired>(patient), Some(&Fired(0)));
}

// --- a failed resume ---------------------------------------------------------

#[derive(Component, Debug, PartialEq)]
struct Log(Vec<&'static str>);

#[derive(Resource, Default)]
struct TopReady(bool);

struct Scout {
    log: Vec<&'static str>,
    top_ready: bool,
    middle_updates: u32,
}

#[derive(QueryData)]
#[query_data(mutable)]
struct ScoutAccess {
    log: &'static mut Log,
    middle: &'static mut Ammo,
}

impl BehaviorContext for Scout {
    type Agent = ScoutAccess;
    type Param = Res<'static, TopReady>;
    type Snapshot = Self;

    fn read(_: Entity, agent: &ScoutAccessItem, top: &Res<TopReady>) -> Scout {
        Scout {
            log: agent.log.0.clone(),
            top_ready: top.0,
            middle_updates: agent.middle.0,
        }
    }

    fn write(scout: &Scout, agent: &mut ScoutAccessItem) {
        if agent.log.0 != scout.log {
            agent.log.0.clone_from(&scout.log);
        }
        agent.middle.set_if_neq(Ammo(scout.middle_updates));
    }
}

/// Priority order, where the middle branch suspends and then fails, and the top
/// one becomes available while it is suspended.
fn scout_tree(fallback: NodeResult) -> impl BehaviorNode<Scout> {
    select((
        seq((
            check(|bb: &Blackboard<Scout>| bb.top_ready),
            leaf(|bb: &mut Blackboard<Scout>| {
                bb.log.push("top");
                NodeResult::Success
            }),
        )),
        leaf(|bb: &mut Blackboard<Scout>| {
            bb.middle_updates += 1;
            bb.log.push("middle");
            if bb.middle_updates >= 2 {
                NodeResult::Failure
            } else {
                NodeResult::Running
            }
        }),
        leaf(move |bb: &mut Blackboard<Scout>| {
            bb.log.push("fallback");
            fallback
        }),
    ))
}

/// No fallback worth the name: when the middle branch fails, so does the tree.
fn failing_fallback() -> impl BehaviorNode<Scout> {
    scout_tree(NodeResult::Failure)
}

fn ending_fallback() -> impl BehaviorNode<Scout> {
    scout_tree(NodeResult::Success)
}

fn running_fallback() -> impl BehaviorNode<Scout> {
    scout_tree(NodeResult::Running)
}
fn scout_app<F: TreeBuilder<Scout> + Copy>(builder: F) -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<TopReady>()
        .add_plugins(BehaviorPlugin::for_tree(builder));
    let agent = app
        .world_mut()
        .spawn((Log(vec![]), Ammo(0), Behavior::for_tree(builder)))
        .id();
    (app, agent)
}

fn log(app: &App, agent: Entity) -> Vec<&'static str> {
    app.world().get::<Log>(agent).unwrap().0.clone()
}

#[test]
fn a_tree_that_fails_on_resume_reconsiders_in_the_same_tick() {
    let (mut app, agent) = scout_app(failing_fallback);

    app.update();
    assert_eq!(
        log(&app, agent),
        ["middle"],
        "suspended in the middle branch"
    );

    app.world_mut().resource_mut::<TopReady>().0 = true;
    app.update();

    // Everything below the resumed branch failed too, so the whole invocation
    // did -- and a failure reached from a resume says nothing about what the
    // tree would choose now, because nothing above the resumed branch was
    // consulted. The same tick re-enters from the root, which spares the agent
    // a tick of doing nothing.
    assert_eq!(
        log(&app, agent),
        ["middle", "middle", "fallback", "top"],
        "the retry ran the branch that became available"
    );
}

#[test]
fn a_fallback_that_succeeds_below_a_failed_resume_keeps_priority_down() {
    let (mut app, agent) = scout_app(ending_fallback);

    app.update();
    app.world_mut().resource_mut::<TopReady>().0 = true;
    app.update();

    // The tree did not fail -- the fallback below the resumed branch succeeded
    // -- so there is nothing for the retry to catch. The agent spends this tick
    // on the lower-priority branch, and the next invocation, which starts
    // fresh, picks the top one.
    assert_eq!(log(&app, agent), ["middle", "middle", "fallback"]);
    app.update();
    assert_eq!(log(&app, agent).last(), Some(&"top"));
}

#[test]
fn a_running_fallback_below_a_failed_resume_holds_priority_down_for_good() {
    let (mut app, agent) = scout_app(running_fallback);

    app.update();
    app.world_mut().resource_mut::<TopReady>().0 = true;
    for _ in 0..4 {
        app.update();
    }

    // The fallback goes Running, so the invocation never ends, the retry never
    // fires, and the top branch is never consulted. Resume is honest about
    // resuming, and this is what that honesty costs. `entry_mode` is what a
    // tree shaped like this has to reach for -- the next test.
    let log = log(&app, agent);
    assert!(
        !log.contains(&"top"),
        "the top branch was never consulted again: {log:?}"
    );
    assert_eq!(
        log.iter().filter(|r| **r == "fallback").count(),
        4,
        "resumed into the fallback on every tick: {log:?}"
    );
}

#[test]
fn entry_mode_is_what_recovers_a_running_fallback() {
    let mut app = App::new();
    app.init_resource::<TopReady>().add_plugins(
        BehaviorPlugin::for_tree(running_fallback).entry_mode(|_| EntryMode::Evaluate),
    );
    let agent = app
        .world_mut()
        .spawn((Log(vec![]), Ammo(0), Behavior::for_tree(running_fallback)))
        .id();

    app.update();
    app.world_mut().resource_mut::<TopReady>().0 = true;
    app.update();

    assert_eq!(log(&app, agent).last(), Some(&"top"));
}

// --- write is skipped for a tree that only read ------------------------------

#[derive(Resource, Default)]
struct Writes(u32);

struct Watcher {
    ammo: u32,
}

#[derive(QueryData)]
#[query_data(mutable)]
struct WatcherAccess {
    ammo: &'static mut Ammo,
}

impl BehaviorContext for Watcher {
    type Agent = WatcherAccess;
    type Param = ();
    type Snapshot = Self;

    fn read(_: Entity, agent: &WatcherAccessItem, _: &()) -> Watcher {
        Watcher { ammo: agent.ammo.0 }
    }

    /// Counts its own calls through a static, because `write` has no world.
    fn write(watcher: &Watcher, agent: &mut WatcherAccessItem) {
        WRITES.fetch_add(1, Ordering::Relaxed);
        agent.ammo.set_if_neq(Ammo(watcher.ammo));
    }
}

static WRITES: AtomicUsize = AtomicUsize::new(0);

fn only_looks() -> impl BehaviorNode<Watcher> {
    check(|bb: &Blackboard<Watcher>| bb.ammo > 0)
}

fn spends() -> impl BehaviorNode<Watcher> {
    leaf(|bb: &mut Blackboard<Watcher>| {
        bb.ammo = bb.ammo.saturating_sub(1);
        NodeResult::Success
    })
}

fn watcher_app<F: TreeBuilder<Watcher> + Copy>(builder: F) -> App {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(builder));
    app.world_mut()
        .spawn((Ammo(5), Behavior::for_tree(builder)));
    app
}

/// One test, because `write` counts through a static and tests share a process.
#[test]
fn write_runs_only_for_a_tree_that_took_mut_to_its_snapshot() {
    WRITES.store(0, Ordering::Relaxed);
    let mut app = watcher_app(only_looks);
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(
        WRITES.load(Ordering::Relaxed),
        0,
        "nothing took &mut to the snapshot, so there was nothing to put back"
    );

    let mut app = watcher_app(spends);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(WRITES.load(Ordering::Relaxed), 3, "one per tick that wrote");
}

#[test]
fn a_read_only_tree_leaves_change_detection_alone() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(only_looks))
        .init_resource::<Writes>()
        .add_systems(
            Update,
            (|changed: Query<(), Changed<Ammo>>, mut seen: ResMut<Writes>| {
                seen.0 += changed.iter().count() as u32;
            })
            .after(BehaviorSystems),
        );
    app.world_mut()
        .spawn((Ammo(5), Behavior::for_tree(only_looks)));

    app.update(); // the spawn itself counts as a change
    app.world_mut().resource_mut::<Writes>().0 = 0;
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Writes>().0,
        0,
        "the agent was not marked changed on any of those ticks"
    );
}
