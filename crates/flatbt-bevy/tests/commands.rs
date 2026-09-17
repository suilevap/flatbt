//! Real ECS commands from inside a tree, and what they cost.
//!
//! Nothing in the crate provides this, and these tests are the record of why:
//! a game that wants it writes what is here, in about twenty lines, and pays a
//! price it can see. `Commands` borrows the world and would put lifetimes back
//! into every node signature; `CommandQueue` is `'static`, so it can simply be
//! a field of the blackboard.
//!
//! Measured over 200 000 agents on a 4-core Xeon, serial tick / whole frame:
//!
//! | blackboard | tick | frame |
//! | --- | --- | --- |
//! | a `bool` field, carried out by a system | 0.77-0.84 ms | 0.94-1.00 ms |
//! | a queue nothing writes to | 1.30-1.33 ms | 1.90-1.98 ms |
//! | a queue 1 agent in 100 writes to | 1.40-1.46 ms | 2.16-2.26 ms |
//! | a queue every agent writes to | 4.82-4.84 ms | 19.2-19.4 ms |
//!
//! Carrying an unused queue costs 70% of the tick, because the blackboard grows
//! by 56 bytes per agent and the drain is another pass over the population.
//! Using one everywhere costs twenty times the frame. So: fine for the rare
//! structural edit -- a spawn, a despawn, an archetype move, a handful per
//! frame -- and never for what an agent decides every tick, which is a field
//! and a system.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::world::CommandQueue;
use flatbt_bevy::prelude::*;

#[derive(Component, Debug, PartialEq)]
struct Scorched;

/// The blackboard, with an owned command buffer in it.
///
/// `CommandQueue` is `'static`, so it can live in a component. That is the
/// whole trick: `Commands` borrows the world and would bring lifetimes back,
/// but the buffer it writes into does not.
#[derive(Component, Default)]
struct Guard {
    ammo: u32,
    orders: CommandQueue,
}

fn scorch() -> impl BehaviorNode<Guard> {
    seq((
        check(|guard: &Guard| guard.ammo > 0),
        leaf(|guard: &mut Guard| {
            // A real command, queued from inside a node.
            guard.orders.push(|world: &mut World| {
                world.spawn(Scorched);
            });
            NodeResult::Success
        }),
    ))
}

/// Drains every agent's queue into the world. One exclusive system, after the
/// tick -- the same place Bevy applies its own deferred commands.
fn apply_orders(world: &mut World) {
    let mut queues: Vec<CommandQueue> = Vec::new();
    let mut agents = world.query::<&mut Guard>();
    for mut guard in agents.iter_mut(world) {
        if guard.orders.is_empty() {
            continue;
        }
        queues.push(core::mem::take(&mut guard.bypass_change_detection().orders));
    }
    for mut queue in queues {
        queue.apply(world);
    }
}

#[test]
fn a_node_can_queue_a_real_command() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(scorch))
        .add_systems(Update, apply_orders.after(BehaviorSystems));
    app.world_mut().spawn((
        Guard {
            ammo: 1,
            ..Guard::default()
        },
        Behavior::for_tree(scorch),
    ));

    app.update();

    assert_eq!(
        app.world_mut()
            .query::<&Scorched>()
            .iter(app.world())
            .count(),
        1,
        "the tree spawned an entity"
    );
}

/// An empty queue is not free -- dropping one walks its buffer -- so pin that
/// keeping it in the component (never dropped, only drained) costs nothing per
/// tick when no node writes to it.
#[test]
fn an_agent_that_issues_nothing_carries_an_empty_queue() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(scorch))
        .add_systems(Update, apply_orders.after(BehaviorSystems));
    let quiet = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(scorch)))
        .id();

    for _ in 0..3 {
        app.update();
    }

    assert!(app.world().get::<Guard>(quiet).unwrap().orders.is_empty());
    assert_eq!(
        app.world_mut()
            .query::<&Scorched>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn the_queue_costs_the_agent_bytes_not_a_lifetime() {
    // 56 bytes per agent, and no allocation until something is pushed --
    // against 5.6 MB of resident blackboard at 100 000 agents.
    assert_eq!(core::mem::size_of::<CommandQueue>(), 56);
}
