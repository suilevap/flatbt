//! What a tree decides, as the payload of `Running`.
//!
//! An act cannot exist without something running, and nothing can run without
//! saying what it is doing. The act type unifies from the nodes that decide and
//! is never written out; nodes that never occupy the agent never name it.

use flatbt::{
    BtAction, BtNode, BtState, EntryMode, NodeResult, action, check, choose, leaf, select, seq,
    update,
};

#[derive(Default)]
struct Fighter {
    ammo: u32,
    rounds_wanted: u32,
    player: f32,
    position: f32,
}

/// What the agent is doing. The game's vocabulary, not the library's.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    MoveTo(f32),
    Reloading,
}

/// Signals a reload and waits for the world to finish it. The tree neither
/// fills the magazine nor knows how long that takes.
struct Reload;

impl BtAction<Fighter, Act> for Reload {
    type State = ();

    fn start(&self, _: &mut Fighter, _: ()) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), fighter: &Fighter, _: ()) -> bool {
        fighter.ammo < fighter.rounds_wanted
    }

    fn tick(&self, _: &mut (), _: &mut Fighter, _: ()) -> Act {
        Act::Reloading
    }
}

/// A long action that follows a moving target: it restates where it is going
/// every update without ending.
struct Chase;

impl BtAction<Fighter, Act> for Chase {
    type State = ();

    fn start(&self, _: &mut Fighter, _: ()) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), fighter: &Fighter, _: ()) -> bool {
        (fighter.position - fighter.player).abs() > 1.0
    }

    fn tick(&self, _: &mut (), fighter: &mut Fighter, _: ()) -> Act {
        Act::MoveTo(fighter.player)
    }
}

/// `Act` is nowhere in this signature's construction -- it unifies from the
/// actions, and `check` never names it.
fn fighter() -> impl BtNode<Fighter, Act> {
    select((
        seq((
            check(|f: &Fighter| f.ammo < f.rounds_wanted),
            action(Reload),
        )),
        action(Chase),
    ))
}

#[test]
fn a_running_tree_says_what_the_agent_is_doing() {
    let tree = fighter();
    let mut state = BtState::new(&tree);
    let mut fighter = Fighter {
        ammo: 6,
        rounds_wanted: 6,
        player: 100.0,
        position: 0.0,
    };

    let doing = update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act();
    assert_eq!(doing, Some(Act::MoveTo(100.0)));
}

#[test]
fn a_long_action_restates_its_act_without_ending() {
    let tree = fighter();
    let mut state = BtState::new(&tree);
    let mut fighter = Fighter {
        ammo: 6,
        rounds_wanted: 6,
        player: 100.0,
        position: 0.0,
    };

    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act(),
        Some(Act::MoveTo(100.0))
    );

    // The target moves. A resume follows it without re-deciding the branch.
    fighter.player = 120.0;
    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Resume).act(),
        Some(Act::MoveTo(120.0))
    );
}

#[test]
fn a_tree_that_ended_is_doing_nothing() {
    let tree = fighter();
    let mut state = BtState::new(&tree);
    let mut fighter = Fighter {
        ammo: 6,
        rounds_wanted: 6,
        player: 0.0,
        position: 0.0,
    };

    // Already there: the chase completes, so the invocation ended.
    let result = update(&tree, &mut state, &mut fighter, EntryMode::Evaluate);
    assert_eq!(result, NodeResult::Success);
    assert_eq!(result.act(), None, "a finished agent is not doing anything");
}

/// The act comes from whichever branch actually ran, and an action that
/// finishes ends the invocation -- so that update decides nothing, and the next
/// one starts fresh.
#[test]
fn an_action_that_finishes_ends_the_invocation_and_decides_nothing() {
    let tree = fighter();
    let mut state = BtState::new(&tree);
    let mut fighter = Fighter {
        ammo: 0,
        rounds_wanted: 6,
        player: 120.0,
        position: 0.0,
    };

    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act(),
        Some(Act::Reloading)
    );

    // The world filled the magazine, so the reload is done. The branch
    // succeeds, and a tree that ended is doing nothing.
    fighter.ammo = 6;
    let result = update(&tree, &mut state, &mut fighter, EntryMode::Evaluate);
    assert_eq!(result, NodeResult::Success);
    assert_eq!(result.act(), None);

    // The next update is a fresh invocation, which picks the chase.
    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act(),
        Some(Act::MoveTo(120.0))
    );
}

/// A candidate that fails leaves the branch that was already running in place,
/// and the act comes from that branch.
#[test]
fn a_failed_candidate_keeps_the_standing_act() {
    let tree = fighter();
    let mut state = BtState::new(&tree);
    let mut fighter = Fighter {
        ammo: 6,
        rounds_wanted: 6,
        player: 120.0,
        position: 0.0,
    };

    // Chasing, because the reload guard fails.
    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act(),
        Some(Act::MoveTo(120.0))
    );

    // Still chasing after a rescan that again rejects the branch above.
    fighter.position = 10.0;
    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act(),
        Some(Act::MoveTo(120.0))
    );
}

#[test]
fn choose_forwards_the_act_of_the_arm_it_picked() {
    let tree = choose!(|f: &Fighter| match f.ammo {
        0 => action(Reload),
        _ => action(Chase),
    });
    let mut state = BtState::new(&tree);
    let mut fighter = Fighter {
        ammo: 0,
        rounds_wanted: 6,
        player: 50.0,
        position: 0.0,
    };

    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act(),
        Some(Act::Reloading)
    );

    fighter.ammo = 6;
    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act(),
        Some(Act::MoveTo(50.0))
    );
}

/// A leaf can decide too, for the case where the decision is instant and the
/// world carries it out. It has to keep the invocation running to say so.
#[test]
fn a_leaf_can_decide() {
    let tree = seq((
        check(|f: &Fighter| f.ammo > 0),
        leaf(|f: &mut Fighter| NodeResult::Running(Act::MoveTo(f.player))),
    ));
    let mut state = BtState::new(&tree);
    let mut fighter = Fighter {
        ammo: 1,
        player: 7.0,
        ..Fighter::default()
    };

    assert_eq!(
        update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act(),
        Some(Act::MoveTo(7.0))
    );
}

/// A tree whose nodes decide nothing keeps working, with `()` as the act type.
#[test]
fn a_tree_that_decides_nothing_still_runs() {
    let tree = seq((
        check(|f: &Fighter| f.ammo > 0),
        leaf(|f: &mut Fighter| {
            f.ammo -= 1;
            NodeResult::Success
        }),
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut fighter = Fighter {
        ammo: 1,
        ..Fighter::default()
    };

    let result = update(&tree, &mut state, &mut fighter, EntryMode::Evaluate);
    assert_eq!(result, NodeResult::Success);
    assert_eq!(fighter.ammo, 0);
}
