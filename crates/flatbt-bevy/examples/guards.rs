//! Guards patrolling a corridor until an alarm sends them at an intruder.
//!
//! Shows one context shared by many agents, read-only shared world access,
//! deferred commands from a node, and state that survives across ticks. The tree
//! is built once into a resource; agents carry only their own state.
//!
//! ```sh
//! cargo run -p flatbt-bevy --example guards
//! ```

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action};

#[derive(Component, Debug)]
struct Post(f32);

#[derive(Component, Debug)]
struct Ammo(u32);

#[derive(Component, Debug)]
struct Name(&'static str);

/// Inserted by the tree through commands; read by an ordinary Bevy system.
#[derive(Component)]
struct Firing;

#[derive(Resource)]
struct Alarm {
    raised: bool,
    intruder: f32,
}

/// Everything the guard trees may touch. Declared once, not per node.
#[derive(QueryData)]
#[query_data(mutable)]
struct Guard {
    name: &'static Name,
    post: &'static mut Post,
    ammo: &'static mut Ammo,
}

impl BehaviorContext for Guard {
    type Agent = Self;
    type Param = Res<'static, Alarm>;
}

/// Reloads over several ticks: start, stay in progress, then finish.
/// The action catalog drives a Bevy context like any other.
struct Reload {
    ticks: u32,
    rounds: u32,
}

impl BtAction<Bt<'_, '_, '_, '_, '_, Guard>> for Reload {
    /// Ticks elapsed so far. Kept between updates; dropped when the action ends.
    type State = u32;

    fn start(&self, bt: &mut Bt<'_, '_, '_, '_, '_, Guard>, _: ()) -> Option<u32> {
        println!("  {} starts reloading", bt.name.0);
        Some(0)
    }

    fn is_in_progress(&self, elapsed: &u32, _: &Bt<'_, '_, '_, '_, '_, Guard>, _: ()) -> bool {
        *elapsed < self.ticks
    }

    fn tick(&self, elapsed: &mut u32, _: &mut Bt<'_, '_, '_, '_, '_, Guard>, _: ()) {
        *elapsed += 1;
    }

    fn complete(&self, _: &mut u32, bt: &mut Bt<'_, '_, '_, '_, '_, Guard>, _: ()) -> bool {
        bt.ammo.0 = self.rounds;
        println!("  {} reloaded", bt.name.0);
        true
    }
}

fn alarm_raised(bt: &Bt<Guard>) -> bool {
    bt.shared.raised
}

fn in_range(bt: &Bt<Guard>) -> bool {
    (bt.post.0 - bt.shared.intruder).abs() <= 1.0
}

fn guard_tree() -> impl BehaviorNode<Guard> {
    select((
        // Shoot while the intruder is close and the magazine holds rounds.
        seq((
            check(alarm_raised),
            check(in_range),
            check(|bt: &Bt<Guard>| bt.ammo.0 > 0),
            leaf(|bt: &mut Bt<Guard>| {
                bt.ammo.0 -= 1;
                let entity = bt.entity;
                bt.commands.entity(entity).insert(Firing);
                println!("  {} fires ({} left)", bt.name.0, bt.ammo.0);
                NodeResult::Success
            }),
        )),
        // Out of ammo: reload, keeping progress across ticks.
        seq((
            check(|bt: &Bt<Guard>| bt.ammo.0 == 0),
            action(Reload {
                ticks: 2,
                rounds: 2,
            }),
        )),
        // Alarm but out of range: close in.
        seq((
            check(alarm_raised),
            leaf(|bt: &mut Bt<Guard>| {
                let step = (bt.shared.intruder - bt.post.0).signum();
                bt.post.0 += step;
                println!("  {} advances to {}", bt.name.0, bt.post.0);
                NodeResult::Success
            }),
        )),
        // Otherwise walk the beat.
        leaf(|bt: &mut Bt<Guard>| {
            bt.post.0 += 1.0;
            println!("  {} patrols to {}", bt.name.0, bt.post.0);
            NodeResult::Success
        }),
    ))
}

/// An ordinary system reacting to what the trees did. `Firing` is inserted by a
/// node and cleared here, so the flag lasts exactly one tick.
fn clear_firing(firing: Query<Entity, With<Firing>>, mut commands: Commands) {
    for entity in firing.iter() {
        commands.entity(entity).remove::<Firing>();
    }
}

fn main() {
    let mut app = App::new();
    app.insert_resource(Alarm {
        raised: false,
        intruder: 4.0,
    })
    // One plugin; the tree builds and registers itself with its first agent.
    .add_plugins(FlatBtPlugin::new())
    .add_systems(Update, clear_firing.after(BehaviorSystems));

    // One tree, many agents: each carries only its own invocation state.
    app.world_mut().spawn((
        Name("Ada"),
        Post(0.0),
        Ammo(1),
        Behavior::for_tree(guard_tree),
    ));
    app.world_mut().spawn((
        Name("Brun"),
        Post(6.0),
        Ammo(2),
        Behavior::for_tree(guard_tree),
    ));

    for tick in 1..=7 {
        if tick == 3 {
            println!("-- alarm raised --");
            app.world_mut().resource_mut::<Alarm>().raised = true;
        }
        println!("tick {tick}");
        app.update();
    }
}
