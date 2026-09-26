use std::sync::{Arc, Mutex};

use flatbt::prelude::*;

#[derive(Default)]
struct Needs {
    hunger: f32,
    fatigue: f32,
    boredom: f32,
    /// Which children refuse to start.
    refuses: [bool; 3],
    starts: Vec<&'static str>,
    /// Children dropped while running: cancelled.
    cancelled: Arc<Mutex<Vec<&'static str>>>,
}

/// Runs until dropped, recording when it starts and when it is cancelled.
struct Doing(&'static str, usize);

struct Busy {
    name: &'static str,
    cancelled: Arc<Mutex<Vec<&'static str>>>,
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.cancelled.lock().unwrap().push(self.name);
    }
}

impl BtAction<Needs, &'static str> for Doing {
    type State = Busy;

    fn start(&self, needs: &mut Needs, _: ()) -> Option<Busy> {
        if needs.refuses[self.1] {
            return None;
        }
        needs.starts.push(self.0);
        Some(Busy {
            name: self.0,
            cancelled: needs.cancelled.clone(),
        })
    }

    fn is_in_progress(&self, _: &Busy, _: &Needs, _: ()) -> bool {
        true
    }

    fn tick(&self, _: &mut Busy, _: &mut Needs, _: ()) -> &'static str {
        self.0
    }
}

fn needs() -> impl BtNode<Needs, &'static str> {
    utility!(|needs: &Needs| {
        needs.hunger => action(Doing("eat", 0)),
        needs.fatigue => action(Doing("sleep", 1)),
        needs.boredom => action(Doing("play", 2)),
    })
}

#[test]
fn runs_the_best_child_and_resume_keeps_it() {
    let tree = needs();
    let mut state = BtState::new(&tree);
    let mut needs = Needs {
        hunger: 0.2,
        fatigue: 0.9,
        boredom: 0.5,
        ..Needs::default()
    };

    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("sleep")
    );
    needs.hunger = 1.0;
    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Resume).act(),
        Some("sleep")
    );
    assert_eq!(needs.starts, ["sleep"]);
}

#[test]
fn a_better_child_preempts_under_evaluate_and_cancels_the_running_one() {
    let tree = needs();
    let mut state = BtState::new(&tree);
    let mut needs = Needs {
        hunger: 0.2,
        fatigue: 0.9,
        ..Needs::default()
    };

    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("sleep")
    );
    needs.hunger = 1.0;
    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("eat")
    );
    assert_eq!(needs.starts, ["sleep", "eat"]);
    assert_eq!(*needs.cancelled.lock().unwrap(), ["sleep"]);
}

#[test]
fn a_failed_child_hands_over_to_the_best_untried_one() {
    let tree = needs();
    let mut state = BtState::new(&tree);
    let mut needs = Needs {
        hunger: 0.9,
        fatigue: 0.5,
        boredom: 0.1,
        refuses: [true, true, false],
        ..Needs::default()
    };

    // eat and sleep refuse in score order; play is all that is left.
    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("play")
    );

    needs.refuses = [true, true, true];
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate),
        NodeResult::Failure
    );
}

#[test]
fn inertia_keeps_the_running_child_until_a_challenger_clears_it() {
    let tree = utility!(|needs: &Needs| {
        needs.hunger => action(Doing("eat", 0)),
        needs.fatigue => action(Doing("sleep", 1)),
    }, inertia = 0.3);
    let mut state = BtState::new(&tree);
    let mut needs = Needs {
        hunger: 0.2,
        fatigue: 0.5,
        ..Needs::default()
    };

    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("sleep")
    );
    needs.hunger = 0.7;
    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("sleep")
    );
    needs.hunger = 0.9;
    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("eat")
    );
}

#[test]
fn nan_skips_a_child_and_ties_go_to_the_first() {
    let tree = needs();
    let mut state = BtState::new(&tree);
    let mut needs = Needs {
        hunger: f32::NAN,
        fatigue: 0.4,
        boredom: 0.4,
        ..Needs::default()
    };

    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("sleep")
    );

    needs.fatigue = f32::NAN;
    needs.boredom = f32::NAN;
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate),
        NodeResult::Failure
    );
}

#[test]
fn integer_scores_make_a_dynamic_priority_selector() {
    let tree = utility(
        |alarm: &u8, index: usize| match index {
            0 => *alarm,
            _ => 1,
        },
        (
            leaf(|_: &mut u8| NodeResult::Running("fight")),
            leaf(|_: &mut u8| NodeResult::Running("patrol")),
        ),
    );
    let mut state = BtState::new(&tree);
    let mut alarm = 0;

    assert_eq!(
        update(&tree, &mut state, &mut alarm, EntryMode::Evaluate).act(),
        Some("patrol")
    );
    alarm = 5;
    assert_eq!(
        update(&tree, &mut state, &mut alarm, EntryMode::Evaluate).act(),
        Some("fight")
    );
}

#[test]
fn inertia_without_the_macro() {
    let tree = select(order_by(
        by_score(|n: &u8, index: usize| if index == 0 { *n } else { 3 }).inertia(2),
        (
            leaf(|_: &mut u8| NodeResult::Running("a")),
            leaf(|_: &mut u8| NodeResult::Running("b")),
        ),
    ));
    let mut state = BtState::new(&tree);
    let mut n = 0;

    assert_eq!(
        update(&tree, &mut state, &mut n, EntryMode::Evaluate).act(),
        Some("b")
    );
    n = 5;
    assert_eq!(
        update(&tree, &mut state, &mut n, EntryMode::Evaluate).act(),
        Some("b")
    );
    n = 6;
    assert_eq!(
        update(&tree, &mut state, &mut n, EntryMode::Evaluate).act(),
        Some("a")
    );
}

#[test]
fn a_tie_keeps_the_running_child() {
    let tree = needs();
    let mut state = BtState::new(&tree);
    let mut needs = Needs {
        hunger: 0.1,
        fatigue: 0.5,
        ..Needs::default()
    };

    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("sleep")
    );
    needs.hunger = 0.5;
    assert_eq!(
        update(&tree, &mut state, &mut needs, EntryMode::Evaluate).act(),
        Some("sleep")
    );
    assert_eq!(needs.starts, ["sleep"]);
}

#[test]
fn a_sequence_by_score_runs_every_child_best_first() {
    let order = |_: &Vec<&str>, index: usize| [1, 3, 2][index];
    let step = |name: &'static str| {
        leaf(move |log: &mut Vec<&'static str>| {
            log.push(name);
            NodeResult::Success
        })
    };
    let tree = seq(order_by(
        by_score(order),
        (step("low"), step("high"), step("mid")),
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut log = Vec::new();

    assert_eq!(
        update(&tree, &mut state, &mut log, EntryMode::Evaluate),
        NodeResult::Success
    );
    assert_eq!(log, ["high", "mid", "low"]);
}

// --- random orders -------------------------------------------------------------

/// A scripted generator: each draw takes the next value.
#[derive(Default)]
struct Dice {
    draws: Vec<u32>,
    ran: Vec<usize>,
    fails: [bool; 4],
}

fn roll(dice: &mut Dice) -> u32 {
    dice.draws.remove(0)
}

fn die(index: usize) -> impl BtNode<Dice, usize> {
    leaf(move |dice: &mut Dice| {
        dice.ran.push(index);
        if dice.fails[index] {
            NodeResult::Failure
        } else {
            NodeResult::Running(index)
        }
    })
}

fn shuffled_select() -> impl BtNode<Dice, usize> {
    random_select(roll, (die(0), die(1), die(2), die(3)))
}

#[test]
fn a_shuffled_select_draws_each_position_from_the_untried_children() {
    let tree = shuffled_select();
    let mut dice = Dice {
        // 2 % 4 -> 2; 2 % 3 of [0, 1, 3] -> 3; 1 % 2 of [0, 1] -> 1; then 0.
        draws: vec![2, 2, 1, 7],
        fails: [true; 4],
        ..Dice::default()
    };
    let mut state = BtState::new(&tree);

    assert_eq!(
        update(&tree, &mut state, &mut dice, EntryMode::Evaluate),
        NodeResult::Failure
    );
    assert_eq!(dice.ran, [2, 3, 1, 0]);
    assert!(dice.draws.is_empty(), "one draw per position");
}

#[test]
fn a_running_random_child_is_kept_first_under_evaluate() {
    let tree = shuffled_select();
    let mut dice = Dice {
        // 1 % 4 -> 1, which fails; 1 % 3 of [0, 2, 3] -> 2, which runs.
        draws: vec![1, 1],
        fails: [false, true, false, false],
        ..Dice::default()
    };
    let mut state = BtState::new(&tree);

    assert_eq!(
        update(&tree, &mut state, &mut dice, EntryMode::Evaluate),
        NodeResult::Running(2)
    );
    // Evaluate restarts the pass with the running child first: no draw, no jitter.
    assert_eq!(
        update(&tree, &mut state, &mut dice, EntryMode::Evaluate),
        NodeResult::Running(2)
    );
    assert_eq!(
        update(&tree, &mut state, &mut dice, EntryMode::Resume),
        NodeResult::Running(2)
    );
    assert_eq!(dice.ran, [1, 2, 2, 2]);

    // When it fails, the rest of the pass is drawn afresh.
    dice.fails[2] = true;
    dice.draws = vec![0, 0];
    dice.ran.clear();
    assert_eq!(
        update(&tree, &mut state, &mut dice, EntryMode::Evaluate),
        NodeResult::Running(0)
    );
    assert_eq!(dice.ran, [2, 0]);
}

#[test]
fn a_shuffled_sequence_runs_every_child_once() {
    let step = |index: usize| {
        leaf(move |dice: &mut Dice| {
            dice.ran.push(index);
            if dice.fails[index] {
                NodeResult::<usize>::Failure
            } else {
                NodeResult::Success
            }
        })
    };
    let tree = shuffle_seq(roll, (step(0), step(1), step(2), step(3)));
    let mut dice = Dice {
        draws: vec![3, 0, 1, 0],
        ..Dice::default()
    };
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut dice, EntryMode::Evaluate),
        NodeResult::Success
    );
    assert_eq!(dice.ran, [3, 0, 2, 1]);

    // Stops at the first failure.
    dice.draws = vec![3, 0];
    dice.fails[0] = true;
    dice.ran.clear();
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut dice, EntryMode::Evaluate),
        NodeResult::Failure
    );
    assert_eq!(dice.ran, [3, 0]);
}

#[test]
fn weighted_order_follows_weights_and_leaves_out_non_positive_ones() {
    let weights = |_: &Dice, index: usize| [0.0, 1.0, 3.0, f32::NAN][index];
    let tree = weighted_select(roll, weights, (die(0), die(1), die(2), die(3)));

    // Draws map to [0, 4): child 1 covers [0, 1), child 2 covers [1, 4).
    for (draw, first) in [(0, 1), (u32::MAX / 8, 1), (u32::MAX / 2, 2), (u32::MAX, 2)] {
        let mut dice = Dice {
            draws: vec![draw, 0],
            fails: [true; 4],
            ..Dice::default()
        };
        let mut state = BtState::new(&tree);
        assert_eq!(
            update(&tree, &mut state, &mut dice, EntryMode::Evaluate),
            NodeResult::Failure
        );
        // Zero and NaN weights are never drawn; the other child falls back.
        assert_eq!(dice.ran, [first, 3 - first]);
    }
}

#[test]
fn a_control_that_jumps_positions_is_reported_and_fails() {
    // `choose!` asks for any position; an order can only be walked in turn.
    let tree = control(
        Choose(|_: &Dice| 2),
        order_by(shuffled(roll), (die(0), die(1), die(2))),
    );
    let mut state = BtState::new(&tree);
    let mut dice = Dice::default();

    assert_eq!(
        update(&tree, &mut state, &mut dice, EntryMode::Evaluate),
        NodeResult::Failure
    );
    assert!(dice.ran.is_empty());
}
