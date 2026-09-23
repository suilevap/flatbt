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
fn inertia_through_the_function_api() {
    let tree = control(
        Utility::new(|n: &u8, index: usize| if index == 0 { *n } else { 3 }).inertia(2),
        (
            leaf(|_: &mut u8| NodeResult::Running("a")),
            leaf(|_: &mut u8| NodeResult::Running("b")),
        ),
    );
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
