//! Guards patrolling a corridor until an alarm sends them at an intruder.
//!
//! Shows the whole shape of the integration: one component is the blackboard,
//! an ordinary system gathers into it, the tree decides, and ordinary systems
//! carry the decision out. The tree is built once into a resource; agents carry
//! only their own invocation state.
//!
//! ```sh
//! cargo run -p flatbt-bevy --example guards
//! ```

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action};

/// The game's truth about a guard. The tree never writes these.
#[derive(Component, Debug, PartialEq)]
struct Post(f32);

#[derive(Component, Debug, PartialEq)]
struct Ammo(u32);

#[derive(Component, Debug)]
struct Name(&'static str);

/// Written by a system acting on the tree's decision; read by another system.
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

/// The blackboard: everything the tree may read, and everything it decides.
///
/// One component, so the tick's query is `(&mut Behavior, &mut Guard)` and
/// nothing else -- disjoint per entity, which is why `.parallel()` needs no
/// declaration. The two halves are not the same data. The top half is gathered
/// from the world before the tick. The bottom half is the tree's output, and it
/// is deliberately not a copy of the top: a tree that moved `post` itself would
/// be deciding how far a guard walks in a tick, and one that subtracted a round
/// would be deciding what a shot costs. It says where it wants to be and that
/// it wants to fire; `carry_out_orders` owns what those mean.
#[derive(Component, Default, Debug)]
struct Guard {
    // Gathered.
    name: &'static str,
    post: f32,
    ammo: u32,
    alarm: bool,
    intruder: f32,
    alarm_changed: bool,
    /// Set by the gather while a march the tree already ordered is still under
    /// way, so the tree is not asked to re-decide a walk in progress.
    marching: bool,
    // Decided.
    march_to: Option<f32>,
    fire: bool,
    reload: bool,
}

/// Fills the blackboard from the world. An ordinary system: it can be split in
/// two, run at a different rate than the tick, or be joined by a second one
/// that fills only the expensive fields -- none of which the tree can tell.
fn gather(mut guards: Query<(&Name, &Post, &Ammo, &mut Guard)>, alarm: Res<Alarm>) {
    for (name, post, ammo, mut guard) in guards.iter_mut() {
        let marching = guard
            .march_to
            .is_some_and(|target| (target - post.0).abs() > 1.0);
        *guard = Guard {
            marching,
            name: name.0,
            post: post.0,
            ammo: ammo.0,
            alarm: alarm.raised,
            intruder: alarm.intruder,
            alarm_changed: alarm.is_changed(),
            // Last tick's decisions are cleared here, so a node that stays
            // silent this tick is not still giving yesterday's order.
            ..Default::default()
        };
    }
}

/// A guard sticks with what it is doing until the alarm itself moves, which is
/// the only thing here worth abandoning a reload for -- and while it is marching
/// somewhere the tree already chose, it is not entered at all.
fn while_the_alarm_holds(guard: &Guard) -> Tick {
    if guard.alarm_changed {
        Tick::Evaluate
    } else if guard.marching {
        // `carry_out_orders` is walking it there; there is nothing to decide
        // until it arrives, and the invocation waits untouched meanwhile.
        Tick::Skip
    } else {
        Tick::Resume
    }
}

/// Reloads over several ticks: start, stay in progress, then finish.
/// The action catalog drives a Bevy blackboard like any other.
struct Reload {
    ticks: u32,
}

impl BtAction<Guard> for Reload {
    /// Ticks elapsed so far. Kept between updates; dropped when the action ends.
    type State = u32;

    fn start(&self, guard: &mut Guard, _: ()) -> Option<u32> {
        println!("  {} starts reloading", guard.name);
        Some(0)
    }

    fn is_in_progress(&self, elapsed: &u32, _: &Guard, _: ()) -> bool {
        *elapsed < self.ticks
    }

    fn tick(&self, elapsed: &mut u32, _: &mut Guard, _: ()) {
        *elapsed += 1;
    }

    fn complete(&self, _: &mut u32, guard: &mut Guard, _: ()) -> bool {
        guard.reload = true;
        true
    }
}

fn alarm_raised(guard: &Guard) -> bool {
    guard.alarm
}

fn in_range(guard: &Guard) -> bool {
    (guard.post - guard.intruder).abs() <= 1.0
}

/// A subtree is a plain function returning a node, so it composes into any tree
/// by being called. Values, not registrations.
fn fire_at_intruder() -> impl BehaviorNode<Guard> {
    seq((
        check(in_range),
        check(|guard: &Guard| guard.ammo > 0),
        leaf(|guard: &mut Guard| {
            guard.fire = true;
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
            check(|guard: &Guard| guard.ammo == 0),
            action(Reload { ticks: 2 }),
        )),
        // Alarm but out of range: close in.
        seq((
            check(alarm_raised),
            leaf(|guard: &mut Guard| {
                guard.march_to = Some(guard.intruder);
                NodeResult::Success
            }),
        )),
        // Otherwise walk the beat.
        leaf(|guard: &mut Guard| {
            guard.march_to = Some(guard.post + 1.0);
            NodeResult::Success
        }),
    ))
}

/// The ordinary system that carries the decision out. Every number here -- how
/// far a guard walks, what a shot costs, how full a magazine is -- belongs to
/// the game, and no tree above ever saw it.
fn carry_out_orders(
    mut guards: Query<(&Guard, &Name, &mut Post, &mut Ammo)>,
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
    .add_plugins(BehaviorPlugin::for_tree(guard_tree).tick_mode(while_the_alarm_holds))
    .add_message::<Fired>()
    // Gather, decide, act: the order is the only thing the integration asks of
    // a game, and it is stated the way Bevy states orders.
    .add_systems(Update, gather.before(BehaviorSystems))
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
        Guard::default(),
        Behavior::for_tree(guard_tree),
    ));
    app.world_mut().spawn((
        Name("Brun"),
        Post(6.0),
        Ammo(2),
        Guard::default(),
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
