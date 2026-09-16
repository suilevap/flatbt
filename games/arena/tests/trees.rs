//! The trees, exercised with no Bevy app at all.
//!
//! This is what the snapshot buys: the context is a plain struct, so a tree is
//! a value that takes a value. No `World`, no schedule, no entities -- just
//! call it and look at what came back.

use core::time::Duration;

use arena::ai::{Fighter, chaser, coward, sniper};
use bevy::prelude::{Entity, Vec2};
use flatbt::bevy::prelude::*;
use flatbt::{BtNode, BtState, EntryMode, update};

fn fighter() -> Fighter {
    Fighter {
        position: Vec2::new(100.0, 0.0),
        health: 100.0,
        ammo: 3,
        speed: 10.0,
        cover: None,
        player: Vec2::ZERO,
        rethink: false,
    }
}

/// Runs `tree` over `snapshot` for `ticks` updates and hands back what it left.
fn run(tree: impl BehaviorNode<Fighter>, snapshot: Fighter, ticks: u32) -> Fighter {
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::<Fighter>::new(Entity::PLACEHOLDER, snapshot);
    for _ in 0..ticks {
        let _ = update(&tree, &mut state, &mut bb, EntryMode::Resume);
    }
    bb.agent
}

#[test]
fn a_chaser_closes_on_the_player() {
    let after = run(chaser(), fighter(), 4);
    assert!(
        after.position.x < 100.0,
        "moved towards the player at the origin, ended at {}",
        after.position
    );
    assert_eq!(after.health, 100.0, "out of reach, so it never swung");
}

#[test]
fn a_chaser_in_reach_swings_instead_of_moving() {
    let close = Fighter {
        position: Vec2::new(10.0, 0.0),
        ..fighter()
    };
    let after = run(chaser(), close, 1);
    assert_eq!(after.position, Vec2::new(10.0, 0.0), "already in reach");
    assert!(after.health < 100.0, "swinging costs it something");
}

#[test]
fn a_sniper_spends_its_magazine_then_reloads() {
    let in_range = Fighter {
        position: Vec2::new(300.0, 0.0),
        ammo: 2,
        ..fighter()
    };
    let after = run(sniper(), in_range, 2);
    assert_eq!(after.ammo, 0, "two shots at two ticks");

    // Dry, it starts a reload, which takes longer than one tick.
    let after = run(sniper(), after, 1);
    assert_eq!(after.ammo, 0, "still reloading");
}

#[test]
fn a_healthy_coward_fights_and_a_hurt_one_asks_for_cover() {
    let after = run(coward(), fighter(), 1);
    assert!(after.position.x < 100.0, "unhurt, so it behaves as a chaser");

    let hurt = Fighter {
        health: 10.0,
        ..fighter()
    };
    let after = run(coward(), hurt, 1);
    assert_eq!(
        after.position,
        Vec2::new(100.0, 0.0),
        "hurt and with nowhere to go yet, it waits on its request"
    );
}

#[test]
fn a_coward_walks_to_the_cover_it_was_given() {
    let hiding = Fighter {
        health: 10.0,
        cover: Some(Vec2::new(0.0, 200.0)),
        ..fighter()
    };
    let after = run(coward(), hiding, 3);
    assert!(
        after.position.distance(Vec2::new(0.0, 200.0)) < fighter().position.distance(Vec2::new(0.0, 200.0)),
        "closed on the spot, ended at {}",
        after.position
    );
}

#[test]
fn evaluate_every_spreads_a_population_across_the_period() {
    let period = Duration::from_millis(100);
    let delta = Duration::from_millis(10);
    let mut evaluated = 0;
    for index in 0..1_000u32 {
        let entity = Entity::from_raw_u32(index).unwrap();
        if evaluate_every(period, Duration::from_millis(500), delta, entity) == EntryMode::Evaluate {
            evaluated += 1;
        }
    }
    // One tick is a tenth of the period, so about a tenth of them.
    assert!(
        (60..140).contains(&evaluated),
        "expected roughly a tenth of 1000, got {evaluated}"
    );
}
