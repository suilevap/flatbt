//! Guards patrolling a corridor until an alarm sends them at an intruder.
//!
//! Shows the whole shape of the integration: a blackboard is gathered from the
//! world, a tree decides what the agent should be *doing*, and each standing
//! decision becomes a component ordinary systems act on. No tree here changes
//! the world; every one of them starts something and waits for the world to
//! finish it.
//!
//! ```sh
//! cargo run -p flatbt-bevy --example guards
//! ```

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action};

// --- the game's own state ----------------------------------------------------

#[derive(Component, Debug, PartialEq)]
struct Post(f32);

#[derive(Component, Debug, PartialEq)]
struct Ammo(u32);

#[derive(Component, Debug)]
struct Name(&'static str);

#[derive(Resource)]
struct Alarm {
    raised: bool,
    intruder: f32,
}

// --- what a guard is doing, as components ------------------------------------
//
// These are the tree's output, and the only interface between it and the rest
// of the game. Nothing below reads `Guard`; everything matches on these.

/// Walking to a place, and where.
#[derive(Component, Debug, PartialEq)]
struct MarchingTo(f32);

/// Refilling the magazine. A marker: how long that takes and how much it puts
/// back belong to `refill`, which is the only thing that knows.
#[derive(Component, Debug, Default, PartialEq)]
struct Reloading;

/// Shooting at the intruder.
#[derive(Component, Debug, Default, PartialEq)]
struct Firing;

/// Fired, once. A message rather than a component, because "this happened" has
/// no duration -- nothing has to clear it and no entity moves archetype.
#[derive(Message)]
struct Shot {
    guard: &'static str,
    left: u32,
}

// --- the blackboard ----------------------------------------------------------

/// What the tree may read, and what it decided.
///
/// The top half is the world as this guard sees it. The bottom half is what it
/// wants, and every field there is carried out to a component below -- so the
/// fields are a detail between the tree and its own plugin registration, not an
/// interface anything else reaches into.
#[derive(Component, Default, Debug)]
struct Guard {
    // Gathered.
    name: &'static str,
    post: f32,
    ammo: u32,
    alarm: bool,
    intruder: f32,
    alarm_changed: bool,
    /// How much of a reload the world still has to do. A reload ends when this
    /// reaches zero, which is `refill`'s business and not the tree's.
    reload_left: u32,
    // Decided.
    march_to: Option<f32>,
    reloading: bool,
    firing: bool,
}

fn gather(mut guards: Query<(&Name, &Post, &Ammo, &mut Guard)>, alarm: Res<Alarm>) {
    for (name, post, ammo, mut guard) in guards.iter_mut() {
        let guard = guard.bypass_change_detection();
        let standing = guard.march_to;
        let reloading = guard.reloading;
        let firing = guard.firing;
        *guard = Guard {
            name: name.0,
            post: post.0,
            ammo: ammo.0,
            alarm: alarm.raised,
            intruder: alarm.intruder,
            alarm_changed: alarm.is_changed(),
            reload_left: 6u32.saturating_sub(ammo.0),
            // A decision already taken stands until the tree revises it.
            march_to: standing,
            reloading,
            firing,
        };
    }
}

/// Reconsider when the alarm moves; otherwise keep what was decided.
///
/// Both branches below `Evaluate` are optimisations: answering `Tick::Evaluate`
/// here every time is always correct and only makes guards quicker to change
/// their minds. `Skip` is worth having because a guard walking somewhere it
/// already chose has nothing to decide until it arrives -- `march` is doing the
/// walking.
fn pace(guard: &Guard) -> Tick {
    if guard.alarm_changed {
        Tick::Evaluate
    } else if walking(guard) {
        Tick::Skip
    } else {
        Tick::Resume
    }
}

// --- the tree ----------------------------------------------------------------

/// Signals a reload and waits for the world to finish it.
///
/// The tree does not know how long a reload takes or how many rounds it puts
/// back: it says "reloading", and `refill` answers by working the magazine up
/// until `reload_left` reaches zero. That is the shape every action here takes.
struct Reload;

impl BtAction<Guard> for Reload {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        println!("  {} starts reloading", guard.name);
        guard.reloading = true;
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.reload_left > 0
    }

    fn complete(&self, _: &mut (), guard: &mut Guard, _: ()) -> bool {
        guard.reloading = false;
        println!("  {} is loaded", guard.name);
        true
    }
}

/// Signals firing and keeps it up while the intruder is in range and there are
/// rounds left. `shoot` is what spends them.
struct FireAt;

impl BtAction<Guard> for FireAt {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.firing = true;
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        in_range(guard) && guard.ammo > 0
    }

    fn complete(&self, _: &mut (), guard: &mut Guard, _: ()) -> bool {
        guard.firing = false;
        true
    }
}

/// Walks somewhere and waits until it is there. `march` moves the guard; how
/// fast, and whether anything is in the way, is the game's.
struct MarchTo {
    where_to: fn(&Guard) -> f32,
}

impl BtAction<Guard> for MarchTo {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.march_to = Some((self.where_to)(guard));
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        walking(guard)
    }

    fn complete(&self, _: &mut (), guard: &mut Guard, _: ()) -> bool {
        guard.march_to = None;
        true
    }
}

/// Still on its way to where it was sent. Read off the world, not off a flag:
/// `march` moves the guard, and this is how the tree notices it arrived.
fn walking(guard: &Guard) -> bool {
    guard
        .march_to
        .is_some_and(|at| (at - guard.post).abs() > 0.5)
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
        action(FireAt),
    ))
}

fn guard_tree() -> impl BehaviorNode<Guard> {
    select((
        // Shoot while the intruder is close and the magazine holds rounds.
        seq((check(alarm_raised), fire_at_intruder())),
        // Out of ammo: reload, and wait for the world to say it is done.
        seq((check(|guard: &Guard| guard.ammo == 0), action(Reload))),
        // Alarm but out of range: close in, and wait until there.
        seq((
            check(alarm_raised),
            action(MarchTo {
                where_to: |guard: &Guard| guard.intruder,
            }),
        )),
        // Otherwise walk the beat.
        action(MarchTo {
            where_to: |guard: &Guard| guard.post + 3.0,
        }),
    ))
}

// --- the systems that do the work --------------------------------------------
//
// Each matches a component the tree put there, and none of them mentions the
// tree, the blackboard, or FlatBT.

fn march(mut guards: Query<(&MarchingTo, &Name, &mut Post)>) {
    for (target, name, mut post) in guards.iter_mut() {
        let step = (target.0 - post.0).clamp(-1.0, 1.0);
        if step != 0.0 {
            post.0 += step;
            println!("  {} marches to {}", name.0, post.0);
        }
    }
}

fn shoot(mut guards: Query<(&Name, &mut Ammo), With<Firing>>, mut shots: MessageWriter<Shot>) {
    for (name, mut ammo) in guards.iter_mut() {
        if ammo.0 == 0 {
            continue;
        }
        ammo.0 -= 1;
        shots.write(Shot {
            guard: name.0,
            left: ammo.0,
        });
    }
}

fn refill(mut guards: Query<&mut Ammo, With<Reloading>>) {
    for mut ammo in guards.iter_mut() {
        ammo.0 = (ammo.0 + 3).min(6);
    }
}

/// An ordinary reader of what happened. Bevy drops read messages on its own, so
/// there is no flag to clear and no archetype to move.
fn report_shots(mut shots: MessageReader<Shot>) {
    for shot in shots.read() {
        println!("  {} fires ({} left)", shot.guard, shot.left);
    }
}

fn main() {
    let mut app = App::new();
    app.insert_resource(Alarm {
        raised: false,
        intruder: 4.0,
    })
    .add_plugins(BehaviorPlugin::for_tree(guard_tree).tick_mode(pace))
    // One line per standing decision. After these, nothing outside this file's
    // tree reads `Guard`.
    .add_plugins((
        ActionComponent::describing(|guard: &Guard| guard.march_to.map(MarchingTo)),
        ActionComponent::<_, Reloading>::while_(|guard: &Guard| guard.reloading),
        ActionComponent::<_, Firing>::while_(|guard: &Guard| guard.firing),
    ))
    .add_message::<Shot>()
    // Gather, decide, turn decisions into components, act.
    .add_systems(Update, gather.before(BehaviorSystems))
    .add_systems(
        Update,
        (march, shoot, refill, report_shots).after(ActionSystems),
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
        Ammo(0),
        Guard::default(),
        Behavior::for_tree(guard_tree),
    ));

    for tick in 1..=8 {
        if tick == 3 {
            println!("-- alarm raised --");
            app.world_mut().resource_mut::<Alarm>().raised = true;
        }
        println!("tick {tick}");
        app.update();
    }
}
