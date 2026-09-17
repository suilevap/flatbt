use arena::plain::{Fighter, chaser, coward, sniper};
use bevy::prelude::Vec2;
use flatbt::bevy::Mind;
use flatbt::{BtState, EntryMode, update};

fn run(tree: impl Mind<Fighter>, mut f: Fighter, ticks: u32) -> Fighter {
    let mut state = BtState::new(&tree);
    for _ in 0..ticks {
        let _ = update(&tree, &mut state, &mut f, EntryMode::Evaluate);
    }
    f
}

fn base() -> Fighter {
    Fighter {
        position: Vec2::new(100.0, 0.0),
        health: 100.0,
        ammo: 3,
        speed: 10.0,
        ..Fighter::default()
    }
}

#[test]
fn a_chaser_heads_for_the_player() {
    assert_eq!(run(chaser(), base(), 1).move_to, Some(Vec2::ZERO));
}

#[test]
fn a_sniper_in_range_shoots_without_spending_the_round() {
    let f = run(sniper(), Fighter { position: Vec2::new(300.0, 0.0), ammo: 2, ..base() }, 1);
    assert!(f.shoot);
    assert_eq!(f.ammo, 2, "the weapon spends it, not the tree");
}

#[test]
fn a_hurt_coward_asks_for_cover_then_walks_to_it() {
    let asked = run(coward(), Fighter { health: 10.0, ..base() }, 1);
    assert!(asked.wants_cover, "no answer yet, so it asks");

    let spot = Vec2::new(0.0, 200.0);
    let walking = run(coward(), Fighter { health: 10.0, cover: Some(spot), ..base() }, 1);
    assert_eq!(walking.move_to, Some(spot));
}
