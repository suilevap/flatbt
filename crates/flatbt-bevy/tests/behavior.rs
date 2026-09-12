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
}

fn shoot() -> impl BehaviorNode<Guard> {
    seq((
        check(|bt: &Bt<Guard>| bt.shared.0),
        check(|bt: &Bt<Guard>| bt.ammo.0 > 0),
        leaf(|bt: &mut Bt<Guard>| {
            bt.ammo.0 -= 1;
            bt.fired.0 += 1;
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
        leaf(|bt: &mut Bt<Guard>| {
            bt.agent_commands().insert(Reloading);
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

impl<C: BehaviorContext> BtNode<Bt<'_, '_, '_, '_, '_, C>> for Recharge {
    type State = u32;

    fn update(
        &self,
        elapsed: &mut u32,
        _: &mut Bt<'_, '_, '_, '_, '_, C>,
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

impl<C: BehaviorContext> BtNode<Bt<'_, '_, '_, '_, '_, C>> for Bulky {
    type State = Trail;

    fn update(
        &self,
        _: &mut Trail,
        _: &mut Bt<'_, '_, '_, '_, '_, C>,
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
            check(|bt: &Bt<Guard>| bt.shared.0),
            leaf(|bt: &mut Bt<Guard>| {
                bt.fired.0 += 1;
                NodeResult::Success
            }),
        )),
        Recharge(10),
    ))
}

#[test]
fn revalidation_is_asked_for_and_spent_once() {
    let mut app = app();
    app.insert_resource(Alarm(false));
    let entity = app
        .world_mut()
        .spawn((Ammo(1), Fired(0), Behavior::for_tree(hold_or_fire)))
        .id();

    // No alarm: the tree settles into the fallback and suspends there.
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));

    // The alarm goes up, but resuming keeps the branch already chosen.
    app.world_mut().resource_mut::<Alarm>().0 = true;
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(0)));

    // The game decides when that decision is stale.
    app.world_mut()
        .entity_mut(entity)
        .insert(BehaviorRevalidate);
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(1)));

    // The request is spent, not sticky.
    assert!(app.world().get::<BehaviorRevalidate>(entity).is_none());
}

fn shoot_then_stop() -> impl BehaviorNode<Guard> {
    seq((
        shoot(),
        leaf(|bt: &mut Bt<Guard>| {
            bt.pause();
            NodeResult::Success
        }),
    ))
}

#[test]
fn a_tree_can_stop_itself() {
    let mut app = app();
    let entity = app
        .world_mut()
        .spawn((Ammo(5), Fired(0), Behavior::for_tree(shoot_then_stop)))
        .id();

    for _ in 0..3 {
        app.update();
    }
    // One tick ran, then the agent paused itself.
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(1)));
    assert!(app.world().get::<BehaviorPaused>(entity).is_some());

    // Pausing is a component, so any system can lift it.
    app.world_mut()
        .entity_mut(entity)
        .remove::<BehaviorPaused>();
    app.update();
    assert_eq!(app.world().get::<Fired>(entity), Some(&Fired(2)));
}

fn advance(step: u32) -> impl BehaviorNode<Guard> {
    leaf(move |bt: &mut Bt<Guard>| {
        bt.fired.0 += step;
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
