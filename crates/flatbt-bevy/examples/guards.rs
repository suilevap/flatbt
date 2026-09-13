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

    /// A guard sticks with what it is doing until the alarm itself moves, which
    /// is the only thing here worth abandoning a reload for.
    fn entry_mode(bb: &Blackboard<Guard>) -> EntryMode {
        if bb.shared.is_changed() {
            EntryMode::Evaluate
        } else {
            EntryMode::Resume
        }
    }
}

/// Reloads over several ticks: start, stay in progress, then finish.
/// The action catalog drives a Bevy context like any other.
struct Reload {
    ticks: u32,
    rounds: u32,
}

impl BtAction<Blackboard<'_, '_, '_, '_, '_, Guard>> for Reload {
    /// Ticks elapsed so far. Kept between updates; dropped when the action ends.
    type State = u32;

    fn start(&self, bb: &mut Blackboard<'_, '_, '_, '_, '_, Guard>, _: ()) -> Option<u32> {
        println!("  {} starts reloading", bb.name.0);
        Some(0)
    }

    fn is_in_progress(
        &self,
        elapsed: &u32,
        _: &Blackboard<'_, '_, '_, '_, '_, Guard>,
        _: (),
    ) -> bool {
        *elapsed < self.ticks
    }

    fn tick(&self, elapsed: &mut u32, _: &mut Blackboard<'_, '_, '_, '_, '_, Guard>, _: ()) {
        *elapsed += 1;
    }

    fn complete(&self, _: &mut u32, bb: &mut Blackboard<'_, '_, '_, '_, '_, Guard>, _: ()) -> bool {
        bb.ammo.0 = self.rounds;
        println!("  {} reloaded", bb.name.0);
        true
    }
}

fn alarm_raised(bb: &Blackboard<Guard>) -> bool {
    bb.shared.raised
}

fn in_range(bb: &Blackboard<Guard>) -> bool {
    (bb.post.0 - bb.shared.intruder).abs() <= 1.0
}

/// A subtree is a plain function returning a node, so it composes into any tree
/// by being called. Values, not registrations.
fn fire_at_intruder() -> impl BehaviorNode<Guard> {
    seq((
        check(in_range),
        check(|bb: &Blackboard<Guard>| bb.ammo.0 > 0),
        leaf(|bb: &mut Blackboard<Guard>| {
            bb.ammo.0 -= 1;
            let entity = bb.entity;
            bb.commands.entity(entity).insert(Firing);
            println!("  {} fires ({} left)", bb.name.0, bb.ammo.0);
            NodeResult::Success
        }),
    ))
}

fn guard_tree() -> impl BehaviorNode<Guard> {
    select((
        // Shoot while the intruder is close and the magazine holds rounds.
        seq((check(alarm_raised), fire_at_intruder())),
        // Out of ammo: reload, keeping progress across ticks.
        seq((
            check(|bb: &Blackboard<Guard>| bb.ammo.0 == 0),
            action(Reload {
                ticks: 2,
                rounds: 2,
            }),
        )),
        // Alarm but out of range: close in.
        seq((
            check(alarm_raised),
            leaf(|bb: &mut Blackboard<Guard>| {
                let step = (bb.shared.intruder - bb.post.0).signum();
                bb.post.0 += step;
                println!("  {} advances to {}", bb.name.0, bb.post.0);
                NodeResult::Success
            }),
        )),
        // Otherwise walk the beat.
        leaf(|bb: &mut Blackboard<Guard>| {
            bb.post.0 += 1.0;
            println!("  {} patrols to {}", bb.name.0, bb.post.0);
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
    // One registration per tree; both type parameters come from the builder.
    .add_plugins(BehaviorPlugin::for_tree(guard_tree))
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
