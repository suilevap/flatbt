//! `ask` and `scope!` over a blackboard that is an ordinary component.
//!
//! The question and the answer are fields, because a tree reads and writes its
//! blackboard and nothing else. What `ask` adds is that the question is put
//! once per invocation rather than once per tick, and that the answer arrives
//! as a scope local -- so the nodes after it take a value, not an `Option`, and
//! the answer does not outlive the decision that wanted it.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;
use flatbt_nodes::ask;
use flatbt_scope::scope;

#[derive(Component, Default, Debug)]
struct Fighter {
    health: f32,
    // The question, and the answer. Both are fields, because the tree has no
    // other way to reach the world.
    wants_cover: bool,
    cover: Option<f32>,
    // What the tree decided.
    move_to: Option<f32>,
}

// --- the node that consumes the local ---------------------------------------

struct WalkTo;

impl BtNode<Fighter, &f32> for WalkTo {
    type State = ();

    fn update(&self, _: &mut (), f: &mut Fighter, spot: &f32, _: EntryMode) -> NodeResult {
        f.move_to = Some(*spot);
        NodeResult::Success
    }
}

fn take_cover() -> impl BehaviorNode<Fighter> {
    scope! {
        let spot: f32;
        sequence {
            ask(
                |f: &mut Fighter| f.wants_cover = true,
                |f: &Fighter| f.cover,
            ).with(out spot);
            WalkTo.with(spot);
        }
    }
}

/// The system that answers. An ordinary gather, at whatever rate it likes.
fn find_cover(mut agents: Query<&mut Fighter>) {
    for mut f in agents.iter_mut() {
        let f = f.bypass_change_detection();
        if f.wants_cover {
            f.cover = Some(42.0);
            f.wants_cover = false;
        }
    }
}

#[test]
fn ask_puts_the_answer_in_a_scope_local_and_the_next_node_takes_a_value() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(take_cover).tick_mode(|_| Tick::Resume))
        .add_systems(Update, find_cover.after(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((Fighter::default(), Behavior::for_tree(take_cover)))
        .id();

    // Tick one: nothing is known, so it asks and waits. The gather answers
    // after the tick.
    app.update();
    let after = app.world().get::<Fighter>(agent).unwrap();
    assert_eq!(after.move_to, None, "nowhere to go yet");
    assert_eq!(after.cover, Some(42.0), "the system answered");

    // Tick two: the answer is there, the action completes, and `WalkTo` gets a
    // plain `f32` rather than an `Option`.
    app.update();
    assert_eq!(
        app.world().get::<Fighter>(agent).unwrap().move_to,
        Some(42.0)
    );
}

/// Asking is once per invocation, not once per tick -- which is the bug the
/// action shape exists to prevent.
#[test]
fn a_waiting_ask_does_not_ask_again_every_tick() {
    #[derive(Resource, Default)]
    struct Asks(u32);

    // Count the asks by never answering.
    let mut app = App::new();
    app.init_resource::<Asks>()
        .add_plugins(BehaviorPlugin::for_tree(take_cover).tick_mode(|_| Tick::Resume))
        .add_systems(
            Update,
            (|mut agents: Query<&mut Fighter>, mut asks: ResMut<Asks>| {
                for mut f in agents.iter_mut() {
                    if f.bypass_change_detection().wants_cover {
                        asks.0 += 1;
                        f.bypass_change_detection().wants_cover = false;
                    }
                }
            })
            .after(BehaviorSystems),
        );
    app.world_mut()
        .spawn((Fighter::default(), Behavior::for_tree(take_cover)));

    for _ in 0..5 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<Asks>().0,
        1,
        "asked once, then waited"
    );
}

/// And the local belongs to the invocation: leaving the branch and coming back
/// asks again rather than walking to a spot picked for an older situation.
#[test]
fn the_local_does_not_outlive_its_invocation() {
    fn hurt_only() -> impl BehaviorNode<Fighter> {
        seq((check(|f: &Fighter| f.health < 40.0), take_cover()))
    }

    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(hurt_only))
        .add_systems(Update, find_cover.after(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((
            Fighter {
                health: 10.0,
                ..Fighter::default()
            },
            Behavior::for_tree(hurt_only),
        ))
        .id();

    app.update();
    app.update();
    assert_eq!(
        app.world().get::<Fighter>(agent).unwrap().move_to,
        Some(42.0)
    );

    // Heal, so the guard fails and the invocation ends; then clear the answer
    // and get hurt again.
    {
        let mut f = app.world_mut().get_mut::<Fighter>(agent).unwrap();
        f.health = 100.0;
        f.cover = None;
        f.move_to = None;
    }
    app.update();
    app.world_mut().get_mut::<Fighter>(agent).unwrap().health = 10.0;
    app.update();

    let after = app.world().get::<Fighter>(agent).unwrap();
    assert_eq!(after.move_to, None, "it asked again rather than reusing");
    assert!(after.cover.is_some(), "and the system is answering again");
}
