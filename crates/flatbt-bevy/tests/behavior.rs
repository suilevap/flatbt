use core::time::Duration;

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

#[derive(QueryData)]
#[query_data(mutable)]
struct Guard {
    ammo: &'static mut Ammo,
    fired: &'static mut Fired,
}

impl BehaviorContext for Guard {
    type Agent = Self;
    type Param = Res<'static, Alarm>;

    /// Standing decisions hold until the alarm itself changes.
    fn entry_mode(bb: &Blackboard<Guard>) -> EntryMode {
        if bb.shared.is_changed() {
            EntryMode::Evaluate
        } else {
            EntryMode::Resume
        }
    }
}

fn shoot() -> impl BehaviorNode<Guard> {
    seq((
        check(|bb: &Blackboard<Guard>| bb.shared.0),
        check(|bb: &Blackboard<Guard>| bb.ammo.0 > 0),
        leaf(|bb: &mut Blackboard<Guard>| {
            bb.ammo.0 -= 1;
            bb.fired.0 += 1;
            NodeResult::Success
        }),
    ))
}

/// The usual setup: one plugin, no registration per tree.
fn app() -> App {
    let mut app = App::new();
    app.insert_resource(Alarm(true))
        .add_plugins(FlatBtPlugin::new());
    app
}

#[test]
fn ticks_agent_components_until_the_guard_fails() {
    let mut app = app();
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
    let mut app = app();
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
    let mut app = app();
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

impl<C: BehaviorContext> BtNode<Blackboard<'_, '_, '_, '_, '_, C>> for Recharge {
    type State = u32;

    fn update(
        &self,
        elapsed: &mut u32,
        _: &mut Blackboard<'_, '_, '_, '_, '_, C>,
        _: (),
        _: EntryMode,
    ) -> NodeResult {
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
    let mut app = app();
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

impl<C: BehaviorContext> BtNode<Blackboard<'_, '_, '_, '_, '_, C>> for Bulky {
    type State = Trail;

    fn update(
        &self,
        _: &mut Trail,
        _: &mut Blackboard<'_, '_, '_, '_, '_, C>,
        _: (),
        _: EntryMode,
    ) -> NodeResult {
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
            check(|bb: &Blackboard<Guard>| bb.shared.0),
            leaf(|bb: &mut Blackboard<Guard>| {
                bb.fired.0 += 1;
                NodeResult::Success
            }),
        )),
        Recharge(10),
    ))
}

#[test]
fn the_context_decides_when_a_standing_decision_is_stale() {
    let mut app = app();
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
    let mut app = app();
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
        bb.fired.0 += step;
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
    let mut app = app();
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
fn a_tree_registers_itself_when_its_first_agent_appears() {
    let mut app = app();
    let entity = app
        .world_mut()
        .spawn((Ammo(2), Fired(0), Behavior::for_tree(shoot)))
        .id();

    // Spawned before the tick schedule runs, so the first frame already ticks it.
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(1)));

    // A second tree needs no registration of its own either.
    let other = app
        .world_mut()
        .spawn((Ammo(0), Fired(0), Behavior::for_tree(reload_when_dry)))
        .id();
    app.update();
    assert!(app.world().get::<Reloading>(other).is_some());
}

fn spawn_a_guard(mut commands: Commands, mut done: Local<bool>) {
    if !*done {
        *done = true;
        commands.spawn((Ammo(2), Fired(0), Behavior::for_tree(shoot)));
    }
}

#[test]
fn an_agent_spawned_mid_tick_starts_on_the_next_frame() {
    let mut app = app();
    app.add_systems(Update, spawn_a_guard);

    // The tick schedule cannot be extended while it runs, so the first agent of
    // a new tree waits a frame. Later agents of that tree tick immediately.
    app.update();
    let fired = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Fired>().single(world).unwrap().0
    };
    assert_eq!(fired(&mut app), 0);

    app.update();
    assert_eq!(fired(&mut app), 1);
}

#[test]
fn an_agent_with_no_plugin_at_all_is_reported() {
    // Neither plugin: nothing would tick this agent, and no tick system queries
    // its component type, so the hook is the only thing that can say so.
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

#[derive(QueryData)]
#[query_data(mutable)]
struct Fighter {
    _turn: &'static Turn,
    journal: &'static mut Journal,
}

impl BehaviorContext for Fighter {
    type Agent = Self;
    type Param = Res<'static, Clock>;
}

/// A turn that spans more than one tick: act, wait, act, then hand the turn on.
fn take_turn() -> impl BehaviorNode<Fighter> {
    seq((
        leaf(|bb: &mut Blackboard<Fighter>| {
            let tick = bb.shared.tick;
            bb.journal.0.push(tick);
            NodeResult::Success
        }),
        Recharge(2),
        leaf(|bb: &mut Blackboard<Fighter>| {
            let tick = bb.shared.tick;
            bb.journal.0.push(tick);
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
        .add_plugins(FlatBtPlugin::new())
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
