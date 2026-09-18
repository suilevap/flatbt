//! What a tree decided, as components the rest of the game matches on.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action};

#[derive(Component, Default)]
struct Guard {
    rounds_left: u32,
    reload_for: u32,
    reloading: bool,
    march_to: Option<f32>,
}

/// Present while the agent is reloading. Nothing but a marker: the work, and
/// what it costs, belongs to the system that matches on it.
#[derive(Component, Default, PartialEq, Debug)]
struct Reloading;

/// Present while the agent is walking somewhere, and says where.
#[derive(Component, PartialEq, Debug)]
struct MarchingTo(f32);

/// Signals a reload and waits for the world to finish it. The tree never
/// touches the magazine: `refill` owns what a reload costs and how long it
/// takes, and says so by clearing `reload_for`.
struct Reload;

impl BtAction<Guard> for Reload {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.reloading = true;
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.reload_for > 0
    }

    fn complete(&self, _: &mut (), guard: &mut Guard, _: ()) -> bool {
        guard.reloading = false;
        true
    }
}

fn reload_when_dry() -> impl BehaviorNode<Guard> {
    seq((
        check(|guard: &Guard| guard.rounds_left == 0),
        action(Reload),
    ))
}

/// The game's own system, matching on the component. It never sees `Guard`.
fn refill(mut agents: Query<&mut Ammo, With<Reloading>>) {
    for mut ammo in agents.iter_mut() {
        ammo.left = (ammo.left + 1).min(6);
    }
}

#[derive(Component, Default)]
struct Ammo {
    left: u32,
}

/// Gathers the world into the blackboard, including how much of the reload is
/// left -- which is what ends the action.
fn gather(mut agents: Query<(&Ammo, &mut Guard)>) {
    for (ammo, mut guard) in agents.iter_mut() {
        let guard = guard.bypass_change_detection();
        guard.rounds_left = ammo.left;
        guard.reload_for = 6u32.saturating_sub(ammo.left);
    }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(reload_when_dry).tick_mode(|_| Tick::Resume))
        .add_plugins(ActionComponent::<_, Reloading>::while_(|g: &Guard| {
            g.reloading
        }))
        .add_systems(Update, gather.before(BehaviorSystems))
        .add_systems(Update, refill.after(ActionSystems));
    app
}

#[test]
fn an_action_puts_a_component_on_the_agent_and_a_system_does_the_work() {
    let mut app = app();
    let agent = app
        .world_mut()
        .spawn((
            Ammo::default(),
            Guard::default(),
            Behavior::for_tree(reload_when_dry),
        ))
        .id();

    // First tick: the tree decides, the bridge inserts, `refill` runs.
    app.update();
    assert!(
        app.world().get::<Reloading>(agent).is_some(),
        "the decision became a component"
    );

    // It takes as many ticks as the world needs, and the tree waits.
    for _ in 0..4 {
        app.update();
    }
    assert!(app.world().get::<Reloading>(agent).is_some());
    assert!(app.world().get::<Ammo>(agent).unwrap().left < 6);

    for _ in 0..4 {
        app.update();
    }
    assert_eq!(app.world().get::<Ammo>(agent).unwrap().left, 6);
    assert!(
        app.world().get::<Reloading>(agent).is_none(),
        "the component goes when the decision does"
    );
}

/// A component carrying a value follows it, without churning archetypes.
#[test]
fn a_describing_component_tracks_the_value() {
    fn march() -> impl BehaviorNode<Guard> {
        leaf(|guard: &mut Guard| {
            guard.march_to = guard.march_to.map(|at| at + 1.0).or(Some(0.0));
            NodeResult::Running
        })
    }

    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(march).tick_mode(|_| Tick::Resume))
        .add_plugins(ActionComponent::describing(|g: &Guard| {
            g.march_to.map(MarchingTo)
        }));
    let agent = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(march)))
        .id();

    app.update();
    assert_eq!(app.world().get::<MarchingTo>(agent), Some(&MarchingTo(0.0)));

    app.update();
    assert_eq!(
        app.world().get::<MarchingTo>(agent),
        Some(&MarchingTo(1.0)),
        "the value follows without a second insert"
    );
}

/// An agent nobody decided anything for carries no component at all, which is
/// the difference between this and a field: a query does not see it.
#[test]
fn an_agent_with_no_standing_decision_carries_nothing() {
    let mut app = app();
    let loaded = app
        .world_mut()
        .spawn((
            Ammo { left: 6 },
            Guard::default(),
            Behavior::for_tree(reload_when_dry),
        ))
        .id();

    for _ in 0..3 {
        app.update();
    }

    assert!(app.world().get::<Reloading>(loaded).is_none());
}
