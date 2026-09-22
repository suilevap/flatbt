//! Guards patrolling a corridor until an alarm sends them at an intruder.
//!
//! The whole shape of the integration: a blackboard gathered from the world, a
//! tree that decides what each guard should be *doing*, and ordinary systems
//! that match the act and do it. No tree here changes the world.
//!
//! One guard is relieved part way through, which is the other half of the
//! contract: stopping an agent takes its standing order back with it.
//!
//! ```sh
//! cargo run -p flatbt-bevy --example guards
//! ```

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;

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

// --- what a guard knows, and what it decides ---------------------------------

/// An aggregate view of the world from one guard's point of view, filled by
/// `gather`. The tree reads it and never writes it.
#[derive(Component, Default, Debug)]
struct Guard {
    name: &'static str,
    post: f32,
    ammo: u32,
    alarm: bool,
    intruder: f32,
    alarm_changed: bool,
    /// How much of a reload the world still has to do. `refill` owns it.
    reload_left: u32,
}

/// What a guard is doing. An order to the world, not a change to it -- and the
/// only thing the tree puts back.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum Act {
    MarchingTo(f32),
    Firing,
    Reloading,
}

fn gather(mut guards: Query<(&Name, &Post, &Ammo, &mut Guard)>, alarm: Res<Alarm>) {
    for (name, post, ammo, mut guard) in guards.iter_mut() {
        *guard = Guard {
            name: name.0,
            post: post.0,
            ammo: ammo.0,
            alarm: alarm.raised,
            intruder: alarm.intruder,
            alarm_changed: alarm.is_changed(),
            reload_left: 6u32.saturating_sub(ammo.0),
        };
    }
}

/// Reconsider when the alarm moves; otherwise carry on with what was decided.
///
/// Both non-`Evaluate` branches are optimisations: answering `Tick::Evaluate`
/// every time is always correct and only makes guards quicker to change their
/// minds. `Skip` pays off because a guard walking somewhere it already chose
/// has nothing to decide until it arrives -- `march` is doing the walking, and
/// the standing act is left in place for it.
fn pace(guard: &Guard, _: TickAt) -> Tick {
    if guard.alarm_changed {
        Tick::Evaluate
    } else {
        Tick::Resume
    }
}

// --- the tree ----------------------------------------------------------------

/// Signals a reload and waits for the world to finish it.
///
/// The tree does not know how long a reload takes or how many rounds it puts
/// back: it says `Reloading` and `refill` answers by working `reload_left` down.
/// Every action here is that shape.
struct Reload;

impl BtAction<Guard, Act> for Reload {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        println!("  {} starts reloading", guard.name);
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.reload_left > 0
    }

    fn tick(&self, _: &mut (), _: &mut Guard, _: ()) -> Act {
        Act::Reloading
    }

    fn complete(&self, _: &mut (), guard: &mut Guard, _: ()) -> bool {
        println!("  {} is loaded", guard.name);
        true
    }
}

/// Keeps firing until its guard stops it.
struct FireAt;

impl BtAction<Guard, Act> for FireAt {
    type State = ();

    fn start(&self, _: &mut Guard, _: ()) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), _: &Guard, _: ()) -> bool {
        true
    }

    fn tick(&self, _: &mut (), _: &mut Guard, _: ()) -> Act {
        Act::Firing
    }
}

/// Walks to wherever the target is *now*. Restating the destination on every
/// update is what lets one standing intent follow a moving intruder.
struct MarchTo {
    where_to: fn(&Guard) -> f32,
}

impl BtAction<Guard, Act> for MarchTo {
    type State = ();

    fn start(&self, _: &mut Guard, _: ()) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        ((self.where_to)(guard) - guard.post).abs() > 0.5
    }

    fn tick(&self, _: &mut (), guard: &mut Guard, _: ()) -> Act {
        Act::MarchingTo((self.where_to)(guard))
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
///
/// A `guard` is asked on every update, so firing stops the moment the intruder
/// leaves range or the magazine runs dry.
fn fire_at_intruder() -> impl BehaviorNode<Guard, Act> {
    guard(
        |guard: &Guard| in_range(guard) && guard.ammo > 0,
        action(FireAt),
    )
}

/// `Act` is declared nowhere but this signature: it unifies from the actions,
/// and the `check`s never name it.
fn guard_tree() -> impl BehaviorNode<Guard, Act> {
    select((
        // Shoot while the intruder is close and the magazine holds rounds.
        seq((check(alarm_raised), fire_at_intruder())),
        // Out of ammo: reload, and wait for the world to say it is done.
        seq((check(|guard: &Guard| guard.ammo == 0), action(Reload))),
        // Alarm but out of range: close in, following the intruder.
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

// --- carrying the act out ----------------------------------------------------

/// Where a guard is headed, or nowhere. Written by `carry_out`, read by
/// `movement` -- an act that belongs to another system is handed over rather
/// than acted on here.
#[derive(Component, Default, Debug, PartialEq)]
struct Destination(Option<f32>);

/// One system, one exhaustive `match`. Adding a variant to `Act` stops this
/// compiling until it is handled, which is the point: with a system per variant
/// a new act is silently ignored.
///
/// What each arm does is the game's. Some acts are done here; `MarchingTo` is
/// delegated, because moving is somebody else's job -- in a real game that arm
/// would write a path request, an animation state, or whatever the subsystem
/// that owns movement reads.
fn carry_out(mut guards: Query<(&Act, &Name, &mut Ammo, &mut Destination)>) {
    for (act, name, mut ammo, mut destination) in guards.iter_mut() {
        // Standing still unless this tick says otherwise. `set_if_neq` keeps a
        // repeated order from marking the component changed.
        let mut headed_for = None;
        match act {
            Act::MarchingTo(target) => headed_for = Some(*target),
            Act::Firing if ammo.0 > 0 => {
                ammo.0 -= 1;
                println!("  {} fires ({} left)", name.0, ammo.0);
            }
            Act::Firing => {}
            Act::Reloading => ammo.0 = (ammo.0 + 3).min(6),
        }
        destination.set_if_neq(Destination(headed_for));
    }
}

/// The subsystem `MarchingTo` was handed to. It knows how far a step is; no
/// tree and no act ever did.
fn movement(mut guards: Query<(&Destination, &Name, &mut Post)>) {
    for (destination, name, mut post) in guards.iter_mut() {
        let Some(target) = destination.0 else {
            continue;
        };
        let step = (target - post.0).clamp(-1.0, 1.0);
        if step != 0.0 {
            post.0 += step;
            println!("  {} marches to {}", name.0, post.0);
        }
    }
}

/// An agent with no act is deciding nothing, which a query can see directly:
/// there is no idle flag to keep in step.
/// The guards with nothing to do: a blackboard and no act.
type Idle<'w, 's> = Query<'w, 's, (&'static Name, &'static mut Destination), Without<Act>>;

fn stand_easy(mut idle: Idle) {
    for (name, mut destination) in idle.iter_mut() {
        destination.set_if_neq(Destination(None));
        println!("  {} stands easy", name.0);
    }
}

fn main() {
    let mut app = App::new();
    app.insert_resource(Alarm {
        raised: false,
        intruder: 4.0,
    })
    .add_plugins(BehaviorPlugin::for_tree(guard_tree).tick_mode(pace))
    // Gather, decide, act. The order is the only thing the integration asks of
    // a game, and it is stated the way Bevy states orders.
    .add_systems(Update, gather.before(BehaviorSystems))
    .add_systems(
        Update,
        (carry_out, stand_easy, movement)
            .chain()
            .after(BehaviorSystems),
    );

    // One tree, many agents: each carries only its own invocation state.
    app.world_mut().spawn((
        Name("Ada"),
        Post(0.0),
        Ammo(1),
        Destination::default(),
        Guard::default(),
        Behavior::for_tree(guard_tree),
    ));
    let brun = app
        .world_mut()
        .spawn((
            Name("Brun"),
            Post(6.0),
            Ammo(0),
            Destination::default(),
            Guard::default(),
            Behavior::for_tree(guard_tree),
        ))
        .id();

    for tick in 1..=8 {
        if tick == 3 {
            println!("-- alarm raised --");
            app.world_mut().resource_mut::<Alarm>().raised = true;
        }
        if tick == 5 {
            // Stopping an agent is removing what made it one. Brun is in the
            // middle of marching at the intruder; the standing order goes with
            // the `Behavior`, so `movement` stops moving him that tick and
            // `stand_easy` picks him up instead.
            println!("-- Brun is relieved --");
            app.world_mut().entity_mut(brun).stop_behavior(guard_tree);
        }
        println!("tick {tick}");
        app.update();
    }
}
