//! [`Split`]: a blackboard whose input half a node cannot write.
//!
//! The point of these is that every constructor FlatBT ships composes over it
//! unchanged, and that no lifetime appears anywhere.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action, choose};
use flatbt_scope::scope;

/// What the world said. A tree may only read this.
#[derive(Default, Debug)]
struct Sensed {
    health: f32,
    ammo: u32,
    cover: Option<f32>,
}

/// What the tree decided. The only thing it may write.
#[derive(Default, Debug, PartialEq)]
struct Orders {
    shoot: bool,
    move_to: Option<f32>,
    wants_cover: bool,
}

type Fighter = Split<Sensed, Orders>;

// --- do the ordinary constructors still work over it? ------------------------

fn shoot() -> impl BehaviorNode<Fighter> {
    seq((
        check(|f: &Fighter| f.ammo > 0),
        leaf(|f: &mut Fighter| {
            f.out().shoot = true;
            NodeResult::Success
        }),
    ))
}

#[test]
fn check_and_leaf_read_the_input_and_write_the_output() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(shoot));
    let agent = app
        .world_mut()
        .spawn((
            Fighter::new(
                Sensed {
                    ammo: 1,
                    ..Sensed::default()
                },
                Orders::default(),
            ),
            Behavior::for_tree(shoot),
        ))
        .id();

    app.update();

    assert!(app.world().get::<Fighter>(agent).unwrap().orders().shoot);
}

// --- actions --------------------------------------------------------------

struct Reload;

impl BtAction<Fighter> for Reload {
    type State = u32;

    fn start(&self, _: &mut Fighter, _: ()) -> Option<u32> {
        Some(0)
    }

    fn is_in_progress(&self, elapsed: &u32, _: &Fighter, _: ()) -> bool {
        *elapsed < 2
    }

    fn tick(&self, elapsed: &mut u32, _: &mut Fighter, _: ()) {
        *elapsed += 1;
    }
}

fn reloading() -> impl BehaviorNode<Fighter> {
    action(Reload)
}

// --- choose ---------------------------------------------------------------

fn either() -> impl BehaviorNode<Fighter> {
    choose!(|f: &Fighter| match f.health < 40.0 {
        true => action(Reload),
        false => shoot(),
    })
}

// --- scope, with a local fed from the input -------------------------------

struct WalkTo;

impl BtNode<Fighter, &f32> for WalkTo {
    type State = ();

    fn update(&self, _: &mut (), f: &mut Fighter, spot: &f32, _: EntryMode) -> NodeResult {
        f.out().move_to = Some(*spot);
        NodeResult::Success
    }
}

fn walk_to_cover() -> impl BehaviorNode<Fighter> {
    scope! {
        let spot: f32 = |f: &mut Fighter| f.cover.unwrap_or_default();
        sequence {
            WalkTo.with(spot);
        }
    }
}

#[test]
fn actions_choose_and_scope_all_compose_over_a_split_blackboard() {
    fn decided<F: TreeBuilder<Fighter> + Copy>(tree: F, sensed: Sensed) -> Orders {
        let mut app = App::new();
        app.add_plugins(BehaviorPlugin::for_tree(tree));
        let agent = app
            .world_mut()
            .spawn((
                Fighter::new(sensed, Orders::default()),
                Behavior::for_tree(tree),
            ))
            .id();
        app.update();
        let mut fighter = app.world_mut().entity_mut(agent).take::<Fighter>().unwrap();
        fighter.take_orders()
    }

    // An action runs and decides nothing yet.
    assert_eq!(decided(reloading, Sensed::default()), Orders::default());

    // `choose!` picks the healthy branch and it shoots.
    assert!(
        decided(
            either,
            Sensed {
                health: 100.0,
                ammo: 1,
                ..Sensed::default()
            }
        )
        .shoot
    );

    // A scope local taken from the input reaches the node under it.
    assert_eq!(
        decided(
            walk_to_cover,
            Sensed {
                cover: Some(7.0),
                ..Sensed::default()
            }
        )
        .move_to,
        Some(7.0)
    );
}

// --- the gather still writes the input -------------------------------------

#[test]
fn a_gather_system_fills_the_read_only_half() {
    fn gather(mut agents: Query<&mut Fighter>) {
        for mut fighter in agents.iter_mut() {
            let fighter = fighter.bypass_change_detection();
            fighter.sensed_mut().ammo = 3;
            *fighter.out() = Orders::default();
        }
    }

    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(shoot))
        .add_systems(Update, gather.before(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((Fighter::default(), Behavior::for_tree(shoot)))
        .id();

    app.update();

    assert!(app.world().get::<Fighter>(agent).unwrap().orders().shoot);
}

/// The rule the type exists for, from the outside: reading is free, writing
/// the output names a method, and writing the input names a method meant for
/// the gather. The `compile_fail` doctest on `Split` is what pins the negative.
#[test]
fn the_input_half_is_read_only_to_a_node() {
    // The `compile_fail` doctest above is the assertion; this pins the shape
    // that does work, so the two stay next to each other.
    let mut fighter = Fighter::new(
        Sensed {
            ammo: 2,
            ..Sensed::default()
        },
        Orders::default(),
    );
    assert_eq!(fighter.ammo, 2, "read through Deref");
    fighter.out().shoot = true;
    assert!(fighter.orders().shoot);
    // And the gather names a method written for it.
    fighter.sensed_mut().ammo = 5;
    assert_eq!(fighter.ammo, 5);
}
