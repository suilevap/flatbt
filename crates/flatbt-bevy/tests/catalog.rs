//! The optional node catalog and scope DSL, over a blackboard that is an
//! ordinary component.
//!
//! Nothing here is Bevy-specific beyond the `#[derive(Component)]`: a tree sees
//! `&mut Guard`, so every node FlatBT ships is written against it directly and
//! composes with the rest. That is the point of the plain design -- the
//! integration adds no type a node has to be generic over.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action, choose};
use flatbt_scope::scope;

#[derive(Component, Default, PartialEq, Debug)]
struct Guard {
    ammo: u32,
    reloads: u32,
}

/// A multi-tick action: `start` runs once, `tick` runs while in progress.
struct Reload;

impl BtAction<Guard> for Reload {
    type State = u32;

    fn start(&self, guard: &mut Guard, _: ()) -> Option<u32> {
        guard.reloads += 1;
        Some(0)
    }

    fn is_in_progress(&self, state: &u32, _: &Guard, _: ()) -> bool {
        *state < 2
    }

    fn tick(&self, state: &mut u32, guard: &mut Guard, _: ()) {
        *state += 1;
        guard.ammo += 1;
    }
}

fn reloading() -> impl BehaviorNode<Guard> {
    action(Reload)
}

/// An action spanning ticks keeps its own state across them, and `start` runs
/// once per invocation rather than once per resume.
#[test]
fn an_action_runs_across_ticks() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(reloading).entry_mode(|_| EntryMode::Resume));
    let entity = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(reloading)))
        .id();

    for _ in 0..3 {
        app.update();
    }

    assert_eq!(
        app.world().get::<Guard>(entity).unwrap(),
        &Guard {
            ammo: 2,
            reloads: 1
        },
        "two ticks of progress under one start"
    );
}

fn picking() -> impl BehaviorNode<Guard> {
    choose!(|guard: &Guard| match guard.ammo {
        0 => action(Reload),
        _ => action(Reload),
    })
}

/// `choose!` reads the blackboard as a plain reference, like any other node.
#[test]
fn choose_selects_on_the_blackboard() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(picking));
    let entity = app
        .world_mut()
        .spawn((Guard::default(), Behavior::for_tree(picking)))
        .id();

    app.update();

    assert_eq!(app.world().get::<Guard>(entity).unwrap().reloads, 1);
}

// --- scope! -----------------------------------------------------------------

/// A node with a parameter: the scope local is handed to it by reference.
struct Aim;

impl BtNode<Guard, &u32> for Aim {
    type State = ();

    fn update(&self, _: &mut (), guard: &mut Guard, target: &u32, _: EntryMode) -> NodeResult {
        guard.ammo = *target;
        NodeResult::Success
    }
}

fn best_target(guard: &mut Guard) -> u32 {
    guard.ammo + 10
}

fn aiming_from_a_function() -> impl BehaviorNode<Guard> {
    scope! {
        let target: u32 = best_target;
        sequence {
            Aim.with(target);
        }
    }
}

fn aiming_from_a_closure() -> impl BehaviorNode<Guard> {
    scope! {
        let target: u32 = |guard: &mut Guard| guard.ammo + 20;
        sequence {
            Aim.with(target);
        }
    }
}

/// A scope local is computed once where the scope is entered, from the same
/// blackboard the rest of the tree reads, and the node under it sees it.
#[test]
fn scope_initializes_locals_from_the_blackboard() {
    fn aimed_at<F: TreeBuilder<Guard> + Copy>(tree: F) -> u32 {
        let mut app = App::new();
        app.add_plugins(BehaviorPlugin::for_tree(tree));
        let entity = app
            .world_mut()
            .spawn((Guard::default(), Behavior::for_tree(tree)))
            .id();

        app.update();

        app.world().get::<Guard>(entity).unwrap().ammo
    }

    assert_eq!(aimed_at(aiming_from_a_function), 10);
    assert_eq!(aimed_at(aiming_from_a_closure), 20);
}
