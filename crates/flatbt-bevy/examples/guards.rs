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

/// What a guard decided this tick. The tree's whole output.
///
/// Not a copy of anything `read` gathered: a tree that moved its own post would
/// be deciding how far a guard walks in a tick, and one that subtracted the
/// round would be deciding what a shot costs. It says where it wants to be and
/// that it wants to fire; `carry_out_orders` owns what those mean.
#[derive(Component, Default, PartialEq)]
struct Orders {
    march_to: Option<f32>,
    fire: bool,
    reload: bool,
}

/// What the guard trees see. Plain data: the tree never touches the ECS.
struct Guard {
    name: &'static str,
    post: f32,
    ammo: u32,
    alarm: bool,
    intruder: f32,
    alarm_changed: bool,
    orders: Orders,
}

/// The access the context may use. Everything `read` needs is `&`; the one
/// thing the tree writes is the only `&mut`, so the split is not a convention.
#[derive(QueryData)]
#[query_data(mutable)]
struct GuardAccess {
    name: &'static Name,
    post: &'static Post,
    ammo: &'static Ammo,
    orders: &'static mut Orders,
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
            orders: Orders::default(),
        }
    }

    fn write(guard: &Guard, agent: &mut GuardAccessItem) {
        agent.orders.set_if_neq(Orders {
            march_to: guard.orders.march_to,
            fire: guard.orders.fire,
            reload: guard.orders.reload,
        });
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
        bb.orders.reload = true;
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
            bb.orders.fire = true;
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
            action(Reload { ticks: 2 }),
        )),
        // Alarm but out of range: close in.
        seq((
            check(alarm_raised),
            leaf(|bb: &mut Blackboard<Guard>| {
                bb.orders.march_to = Some(bb.intruder);
                NodeResult::Success
            }),
        )),
        // Otherwise walk the beat.
        leaf(|bb: &mut Blackboard<Guard>| {
            bb.orders.march_to = Some(bb.post + 1.0);
            NodeResult::Success
        }),
    ))
}

/// The ordinary systems that carry the orders out. Every number here -- how far
/// a guard walks, what a shot costs, how full a magazine is -- belongs to the
/// game, and no tree above ever saw it.
fn carry_out_orders(
    mut guards: Query<(&Orders, &Name, &mut Post, &mut Ammo)>,
    mut fired: MessageWriter<Fired>,
) {
    for (orders, name, mut post, mut ammo) in guards.iter_mut() {
        if let Some(target) = orders.march_to {
            let step = (target - post.0).clamp(-1.0, 1.0);
            if step != 0.0 {
                post.0 += step;
                println!("  {} marches to {}", name.0, post.0);
            }
        }
        if orders.fire && ammo.0 > 0 {
            ammo.0 -= 1;
            fired.write(Fired {
                guard: name.0,
                left: ammo.0,
            });
        }
        if orders.reload {
            ammo.0 = 2;
            println!("  {} reloaded", name.0);
        }
    }
}

/// And an ordinary reader of what happened. Bevy drops read messages on its
/// own, so there is no flag to clear and no archetype to move.
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
    .add_systems(
        Update,
        (carry_out_orders, report_shots)
            .chain()
            .after(BehaviorSystems),
    );

    // One tree, many agents: each carries only its own invocation state.
    app.world_mut().spawn((
        Name("Ada"),
        Post(0.0),
        Ammo(1),
        Orders::default(),
        Behavior::for_tree(guard_tree),
    ));
    app.world_mut().spawn((
        Name("Brun"),
        Post(6.0),
        Ammo(2),
        Orders::default(),
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
