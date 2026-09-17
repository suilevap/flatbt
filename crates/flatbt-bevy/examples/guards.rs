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

#[derive(Component, Debug, PartialEq)]
struct Post(f32);

#[derive(Component, Debug, PartialEq)]
struct Ammo(u32);

#[derive(Component, Debug)]
struct Name(&'static str);

/// Written by the tree; read by an ordinary Bevy system.
///
/// A message rather than a marker component: "this happened" has no duration,
/// so nothing has to clear it, and no entity moves between archetypes to carry
/// it.
#[derive(Message)]
struct Fired {
    guard: &'static str,
    left: u32,
}

#[derive(Resource)]
struct Alarm {
    raised: bool,
    intruder: f32,
}

/// What the guard trees see. Plain data: the tree never touches the ECS.
struct Guard {
    name: &'static str,
    post: f32,
    ammo: u32,
    alarm: bool,
    intruder: f32,
    alarm_changed: bool,
}

/// The access `read` and `write` may use. Declared once, not per node.
#[derive(QueryData)]
#[query_data(mutable)]
struct GuardAccess {
    name: &'static Name,
    post: &'static mut Post,
    ammo: &'static mut Ammo,
}

impl BehaviorContext for Guard {
    type Agent = GuardAccess;
    type Param = Res<'static, Alarm>;
    type Snapshot = Self;

    fn read(_: Entity, agent: &GuardAccessItem, alarm: &Res<Alarm>) -> Guard {
        Guard {
            name: agent.name.0,
            post: agent.post.0,
            ammo: agent.ammo.0,
            alarm: alarm.raised,
            intruder: alarm.intruder,
            alarm_changed: alarm.is_changed(),
        }
    }

    fn write(guard: &Guard, agent: &mut GuardAccessItem) {
        agent.post.set_if_neq(Post(guard.post));
        agent.ammo.set_if_neq(Ammo(guard.ammo));
    }

    /// A guard sticks with what it is doing until the alarm itself moves, which
    /// is the only thing here worth abandoning a reload for.
    fn entry_mode(bb: &Blackboard<Guard>) -> EntryMode {
        if bb.alarm_changed {
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

impl BtAction<Blackboard<Guard>> for Reload {
    /// Ticks elapsed so far. Kept between updates; dropped when the action ends.
    type State = u32;

    fn start(&self, bb: &mut Blackboard<Guard>, _: ()) -> Option<u32> {
        println!("  {} starts reloading", bb.name);
        Some(0)
    }

    fn is_in_progress(&self, elapsed: &u32, _: &Blackboard<Guard>, _: ()) -> bool {
        *elapsed < self.ticks
    }

    fn tick(&self, elapsed: &mut u32, _: &mut Blackboard<Guard>, _: ()) {
        *elapsed += 1;
    }

    fn complete(&self, _: &mut u32, bb: &mut Blackboard<Guard>, _: ()) -> bool {
        bb.ammo = self.rounds;
        println!("  {} reloaded", bb.name);
        true
    }
}

fn alarm_raised(bb: &Blackboard<Guard>) -> bool {
    bb.alarm
}

fn in_range(bb: &Blackboard<Guard>) -> bool {
    (bb.post - bb.intruder).abs() <= 1.0
}

/// A subtree is a plain function returning a node, so it composes into any tree
/// by being called. Values, not registrations.
fn fire_at_intruder() -> impl BehaviorNode<Guard> {
    seq((
        check(in_range),
        check(|bb: &Blackboard<Guard>| bb.ammo > 0),
        leaf(|bb: &mut Blackboard<Guard>| {
            bb.ammo -= 1;
            let fired = Fired {
                guard: bb.name,
                left: bb.ammo,
            };
            bb.write_message(fired);
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
            check(|bb: &Blackboard<Guard>| bb.ammo == 0),
            action(Reload {
                ticks: 2,
                rounds: 2,
            }),
        )),
        // Alarm but out of range: close in.
        seq((
            check(alarm_raised),
            leaf(|bb: &mut Blackboard<Guard>| {
                let step = (bb.intruder - bb.post).signum();
                bb.post += step;
                println!("  {} advances to {}", bb.name, bb.post);
                NodeResult::Success
            }),
        )),
        // Otherwise walk the beat.
        leaf(|bb: &mut Blackboard<Guard>| {
            bb.post += 1.0;
            println!("  {} patrols to {}", bb.name, bb.post);
            NodeResult::Success
        }),
    ))
}

/// An ordinary system reacting to what the trees did. Bevy drops read messages
/// on its own, so there is no flag to clear and no archetype to move.
fn report_shots(mut fired: MessageReader<Fired>) {
    for shot in fired.read() {
        println!("  {} fires ({} left)", shot.guard, shot.left);
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
    .add_message::<Fired>()
    .add_systems(Update, report_shots.after(BehaviorSystems));

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
