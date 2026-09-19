//! The optional node catalog and scope DSL, over a Bevy blackboard and act.
//!
//! Nothing here is Bevy-specific beyond the two `#[derive(Component)]`s: a tree
//! sees `&mut Guard` and returns an `Act`, so every node FlatBT ships is written
//! against them directly.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action, choose};
use flatbt_scope::scope;

#[derive(Component, Default, PartialEq, Debug)]
struct Guard {
    ammo: u32,
    magazine: u32,
    cover: Option<f32>,
}

#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum Act {
    Loading,
    WalkingTo(f32),
}

/// A multi-tick action that says what it is doing on every update, and stops
/// when the world says so.
struct Reload;

impl BtAction<Guard, Act> for Reload {
    type State = ();

    fn start(&self, _: &mut Guard, _: ()) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.ammo < guard.magazine
    }

    fn tick(&self, _: &mut (), _: &mut Guard, _: ()) -> Act {
        Act::Loading
    }
}

fn reloading() -> impl BehaviorNode<Guard, Act> {
    action(Reload)
}

/// The world fills the magazine; the action watches and then lets go.
fn refill(mut guards: Query<&mut Guard, With<Act>>) {
    for mut guard in guards.iter_mut() {
        let guard = guard.bypass_change_detection();
        guard.ammo = (guard.ammo + 1).min(guard.magazine);
    }
}

#[test]
fn an_action_says_what_it_is_doing_until_the_world_is_done() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(reloading).tick_mode(|_| Tick::Resume))
        .add_systems(Update, refill.after(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 0,
                magazine: 3,
                cover: None,
            },
            Behavior::for_tree(reloading),
        ))
        .id();

    app.update();
    assert_eq!(app.world().get::<Act>(agent), Some(&Act::Loading));

    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().get::<Guard>(agent).unwrap().ammo, 3);
    assert_eq!(
        app.world().get::<Act>(agent),
        None,
        "the magazine is full, so the action ended and the act went with it"
    );
}

// --- choose ------------------------------------------------------------------

fn picking() -> impl BehaviorNode<Guard, Act> {
    choose!(|guard: &Guard| match guard.ammo {
        0 => action(Reload),
        _ => action(Reload),
    })
}

#[test]
fn choose_forwards_the_act_of_the_arm_it_picked() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(picking));
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                ammo: 0,
                magazine: 2,
                cover: None,
            },
            Behavior::for_tree(picking),
        ))
        .id();

    app.update();

    assert_eq!(app.world().get::<Act>(agent), Some(&Act::Loading));
}

// --- scope -------------------------------------------------------------------

/// A node with a parameter: the scope local is handed to it by reference, and
/// it decides from that.
struct WalkTo;

impl BtNode<Guard, Act, &f32> for WalkTo {
    type State = ();

    fn update(&self, _: &mut (), _: &mut Guard, spot: &f32, _: EntryMode) -> NodeResult<Act> {
        NodeResult::Running(Act::WalkingTo(*spot))
    }
}

fn walk_to_cover() -> impl BehaviorNode<Guard, Act> {
    scope! {
        let spot: f32 = |guard: &mut Guard| guard.cover.unwrap_or_default();
        sequence {
            WalkTo.with(spot);
        }
    }
}

/// A scope local is computed once where the scope is entered, from the same
/// blackboard the rest of the tree reads, and the node under it decides from it.
#[test]
fn scope_initializes_locals_from_the_blackboard() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(walk_to_cover));
    let agent = app
        .world_mut()
        .spawn((
            Guard {
                cover: Some(7.0),
                ..Guard::default()
            },
            Behavior::for_tree(walk_to_cover),
        ))
        .id();

    app.update();

    assert_eq!(app.world().get::<Act>(agent), Some(&Act::WalkingTo(7.0)));
}
