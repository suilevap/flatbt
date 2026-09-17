//! The trees, exercised with no Bevy app at all.
//!
//! This is what the snapshot buys: the context is a plain struct, so a tree is
//! a value that takes a value. No `World`, no schedule, no entities -- just
//! call it and look at what it decided.
//!
//! "Decided" is the word. These assert on intents, not on consequences: a
//! chaser *asks* to be at the player, and whether it gets there is
//! `apply_movement`'s business and not under test here.

use core::time::Duration;

use arena::ai::{Fighter, chaser, coward, sniper};
use arena::world::Intent;
use bevy::prelude::{Entity, Vec2};
use flatbt::bevy::prelude::*;
use flatbt::{BtState, EntryMode, update};

fn fighter() -> Fighter {
    Fighter {
        position: Vec2::new(100.0, 0.0),
        health: 100.0,
        ammo: 3,
        speed: 10.0,
        cover: None,
        player: Vec2::ZERO,
        rethink: false,
        intent: Intent::default(),
    }
}

/// Runs `tree` over `snapshot` for `ticks` updates and hands back what it left.
fn run(tree: impl BehaviorNode<Fighter>, snapshot: Fighter, ticks: u32) -> Fighter {
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::<Fighter>::new(Entity::PLACEHOLDER, snapshot);
    for _ in 0..ticks {
        let _ = update(&tree, &mut state, &mut bb, EntryMode::Resume);
    }
    bb.into_snapshot()
}

#[test]
fn a_chaser_out_of_reach_heads_for_the_player() {
    let after = run(chaser(), fighter(), 1);
    assert_eq!(
        after.intent.move_to,
        Some(Vec2::ZERO),
        "the player is there"
    );
    assert!(!after.intent.melee, "nothing to swing at yet");
}

#[test]
fn a_chaser_in_reach_swings_instead_of_moving() {
    let close = Fighter {
        position: Vec2::new(10.0, 0.0),
        ..fighter()
    };
    let after = run(chaser(), close, 1);
    assert!(after.intent.melee);
    assert_eq!(after.intent.move_to, None, "already close enough");
}

#[test]
fn a_sniper_in_range_and_loaded_shoots() {
    let in_range = Fighter {
        position: Vec2::new(300.0, 0.0),
        ammo: 2,
        ..fighter()
    };
    let after = run(sniper(), in_range, 1);
    assert!(after.intent.shoot);
    assert_eq!(after.ammo, 2, "spending the round is the weapon's business");
}

#[test]
fn a_dry_sniper_reloads_and_it_takes_more_than_a_tick() {
    let dry = || Fighter {
        position: Vec2::new(300.0, 0.0),
        ammo: 0,
        ..fighter()
    };
    assert!(!run(sniper(), dry(), 1).intent.reload, "still reloading");
    assert!(
        run(sniper(), dry(), 31).intent.reload,
        "finished, and says so once"
    );
}

#[test]
fn a_healthy_coward_fights_and_a_hurt_one_asks_for_cover() {
    let after = run(coward(), fighter(), 1);
    assert_eq!(
        after.intent.move_to,
        Some(Vec2::ZERO),
        "unhurt, so it behaves as a chaser"
    );

    let hurt = Fighter {
        health: 10.0,
        ..fighter()
    };
    let after = run(coward(), hurt, 1);
    assert_eq!(
        after.intent,
        Intent::default(),
        "hurt and with nowhere to go yet, it waits on its request"
    );
}

#[test]
fn a_coward_heads_for_the_cover_it_was_given() {
    let spot = Vec2::new(0.0, 200.0);
    let hiding = Fighter {
        health: 10.0,
        cover: Some(spot),
        ..fighter()
    };
    assert_eq!(run(coward(), hiding, 1).intent.move_to, Some(spot));
}

#[test]
fn evaluate_every_spreads_a_population_across_the_period() {
    let period = Duration::from_millis(100);
    let delta = Duration::from_millis(10);
    let mut evaluated = 0;
    for index in 0..1_000u32 {
        let entity = Entity::from_raw_u32(index).unwrap();
        if evaluate_every(period, Duration::from_millis(500), delta, entity) == EntryMode::Evaluate
        {
            evaluated += 1;
        }
    }
    // One tick is a tenth of the period, so about a tenth of them.
    assert!(
        (60..140).contains(&evaluated),
        "expected roughly a tenth of 1000, got {evaluated}"
    );
}
