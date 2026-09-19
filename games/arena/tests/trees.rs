//! The trees, exercised with no Bevy app at all.
//!
//! The blackboard is a plain struct, so a tree is a value that takes a value:
//! no `World`, no schedule, no entities. What the assertions look at is what
//! the tree *decided* -- `plugin` turns each of those into a component, and the
//! systems that act on them are not under test here.

use core::time::Duration;

use arena::ai::{Fighter, chaser, coward, sniper};
use bevy::prelude::{Entity, Vec2};
use flatbt::bevy::prelude::*;
use flatbt::{BtState, EntryMode, update};

fn fighter() -> Fighter {
    Fighter {
        position: Vec2::new(100.0, 0.0),
        health: 100.0,
        ammo: 3,
        speed: 10.0,
        ..Fighter::default()
    }
}

/// Runs `tree` over a fighter for `ticks` updates and hands back what it left.
fn run(tree: impl BehaviorNode<Fighter>, mut fighter: Fighter, ticks: u32) -> Fighter {
    let mut state = BtState::new(&tree);
    for _ in 0..ticks {
        let _ = update(&tree, &mut state, &mut fighter, EntryMode::Resume);
    }
    fighter
}

#[test]
fn a_chaser_out_of_reach_heads_for_the_player() {
    let after = run(chaser(), fighter(), 1);
    assert_eq!(after.move_to, Some(Vec2::ZERO), "the player is there");
    assert!(!after.meleeing, "nothing to swing at yet");
}

#[test]
fn a_chaser_in_reach_swings_instead_of_moving() {
    let close = Fighter {
        position: Vec2::new(10.0, 0.0),
        ..fighter()
    };
    let after = run(chaser(), close, 1);
    assert!(after.meleeing);
    assert_eq!(after.move_to, None, "already close enough");
}

#[test]
fn a_sniper_in_range_and_loaded_fires() {
    let in_range = Fighter {
        position: Vec2::new(300.0, 0.0),
        ammo: 2,
        ..fighter()
    };
    let after = run(sniper(), in_range, 1);
    assert!(after.firing);
    assert_eq!(
        after.ammo, 2,
        "spending the round is the weapon's business, not the tree's"
    );
}

/// A dry sniper signals a reload and waits for the world to finish it. The tree
/// has no idea how long that takes: `reload_left` is the world's answer.
#[test]
fn a_dry_sniper_reloads_until_the_world_says_it_is_done() {
    let dry = Fighter {
        position: Vec2::new(300.0, 0.0),
        ammo: 0,
        reload_left: 6,
        ..fighter()
    };
    let waiting = run(sniper(), dry, 10);
    assert!(waiting.reloading, "still reloading after ten ticks");

    let finished = run(
        sniper(),
        Fighter {
            reload_left: 0,
            ..waiting
        },
        1,
    );
    assert!(
        !finished.reloading,
        "the world finished it, so the tree let go"
    );
}

#[test]
fn a_healthy_coward_fights_and_a_hurt_one_asks_for_cover() {
    let after = run(coward(), fighter(), 1);
    assert_eq!(
        after.move_to,
        Some(Vec2::ZERO),
        "unhurt, so it behaves as a chaser"
    );

    let hurt = Fighter {
        health: 10.0,
        ..fighter()
    };
    let after = run(coward(), hurt, 1);
    assert!(
        after.cover.is_pending(),
        "hurt with nowhere to go yet, so it asks"
    );
    assert_eq!(after.move_to, None, "and waits where it is");
}

#[test]
fn a_coward_heads_for_the_cover_it_was_given() {
    let spot = Vec2::new(0.0, 200.0);
    let mut fighter = Fighter {
        health: 10.0,
        ..fighter()
    };
    let tree = coward();
    let mut state = BtState::new(&tree);

    let _ = update(&tree, &mut state, &mut fighter, EntryMode::Evaluate);
    assert!(fighter.cover.is_pending());

    // What `find_cover` does, without a `World`.
    fighter.cover.answer(spot);
    let _ = update(&tree, &mut state, &mut fighter, EntryMode::Resume);
    assert_eq!(fighter.move_to, Some(spot));
}

/// The pace is a field, so what a tick does with an agent is a plain predicate
/// over the blackboard and testable the same way.
#[test]
fn the_tick_mode_follows_the_gathered_pace() {
    use arena::ai::pace;

    assert_eq!(pace(&fighter()), Tick::Resume);
    assert_eq!(
        pace(&Fighter {
            rethink: true,
            ..fighter()
        }),
        Tick::Evaluate
    );

    // A walk the tree already ordered and has not finished: nothing to decide,
    // so the tree is not entered and the standing order survives the tick.
    let walking = Fighter {
        move_to: Some(Vec2::new(500.0, 0.0)),
        ..fighter()
    };
    assert_eq!(pace(&walking), Tick::Skip);
    assert_eq!(
        pace(&Fighter {
            move_to: Some(walking.position),
            ..walking
        }),
        Tick::Resume,
        "arrived, so it decides again"
    );
}

#[test]
fn evaluate_every_spreads_a_population_across_the_period() {
    let period = Duration::from_millis(100);
    let delta = Duration::from_millis(10);
    let mut evaluated = 0;
    for index in 0..1_000u32 {
        let entity = Entity::from_raw_u32(index).unwrap();
        if evaluate_every(period, Duration::from_millis(500), delta, entity) == Tick::Evaluate {
            evaluated += 1;
        }
    }
    // One tick is a tenth of the period, so about a tenth of them.
    assert!(
        (60..140).contains(&evaluated),
        "expected roughly a tenth of 1000, got {evaluated}"
    );
}
