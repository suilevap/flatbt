//! An agent's running path, read from the ECS.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;

#[derive(Component, Default)]
struct Guard {
    ammo: u32,
}

#[derive(Component, Clone, Copy, PartialEq)]
enum Act {
    Firing,
    Loading,
}

fn has_ammo(guard: &Guard) -> bool {
    guard.ammo > 0
}

fn shoot() -> impl BehaviorNode<Guard, Act> {
    select((
        guard(
            has_ammo,
            leaf(|_: &mut Guard| NodeResult::Running(Act::Firing)),
        ),
        leaf(|_: &mut Guard| NodeResult::Running(Act::Loading)).named("reload"),
    ))
}

/// Generic over the builder, which names the tree: the only way to name a
/// `Behavior`.
fn describe<C, A, F>(world: &World, agent: Entity, _tree: F) -> String
where
    C: Send + Sync + 'static,
    A: Send + Sync + 'static,
    F: TreeBuilder<C, A>,
{
    let tree = world.resource::<BehaviorTree<C, A, F>>();
    let behavior = world.get::<Behavior<C, A, F>>(agent).unwrap();
    behavior.describe(tree.get()).to_string()
}

#[test]
fn an_agent_describes_its_running_path() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(shoot));
    let agent = app
        .world_mut()
        .spawn((Guard { ammo: 1 }, Behavior::for_tree(shoot)))
        .id();
    assert_eq!(describe(app.world(), agent, shoot), "not running");

    app.update();
    assert_eq!(
        describe(app.world(), agent, shoot),
        "select > has_ammo (guard) > leaf"
    );

    app.world_mut().get_mut::<Guard>(agent).unwrap().ammo = 0;
    app.update();
    assert_eq!(
        describe(app.world(), agent, shoot),
        "select > reload (leaf)"
    );
}
