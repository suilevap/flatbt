use std::sync::{Arc, Mutex};

use flatbt::prelude::*;

#[derive(Default)]
struct Blackboard {
    ran: Vec<&'static str>,
    fail: Vec<&'static str>,
    flag: bool,
    cancelled: Arc<Mutex<Vec<&'static str>>>,
}

/// Records its name; fails if listed in `fail`, otherwise runs one update and
/// succeeds. Dropped before completing, it records a cancellation.
fn task(name: &'static str) -> impl BtNode<Blackboard, &'static str> {
    action(Task(name))
}

struct Task(&'static str);

struct Running {
    name: &'static str,
    ticked: bool,
    done: bool,
    cancelled: Arc<Mutex<Vec<&'static str>>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.done {
            self.cancelled.lock().unwrap().push(self.name);
        }
    }
}

impl BtAction<Blackboard, &'static str> for Task {
    type State = Running;

    fn start(&self, bb: &mut Blackboard, _: ()) -> Option<Running> {
        bb.ran.push(self.0);
        (!bb.fail.contains(&self.0)).then(|| Running {
            name: self.0,
            ticked: false,
            done: false,
            cancelled: bb.cancelled.clone(),
        })
    }

    fn is_in_progress(&self, state: &Running, _: &Blackboard, _: ()) -> bool {
        !state.ticked
    }

    fn tick(&self, state: &mut Running, _: &mut Blackboard, _: ()) -> &'static str {
        state.ticked = true;
        self.0
    }

    fn complete(&self, state: &mut Running, _: &mut Blackboard, _: ()) -> bool {
        state.done = true;
        true
    }
}

fn instant(name: &'static str, succeeds: bool) -> impl BtNode<Blackboard, &'static str> {
    leaf(move |bb: &mut Blackboard| {
        bb.ran.push(name);
        if succeeds {
            NodeResult::Success
        } else {
            NodeResult::Failure
        }
    })
}

#[test]
fn repeat_and_retry_count_within_an_invocation() {
    let tree = repeat(3, task("a"));
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::default();
    let mut updates = 0;
    while update(&tree, &mut state, &mut bb, EntryMode::Resume).is_running() {
        updates += 1;
    }
    assert_eq!((updates, bb.ran.len()), (3, 3));

    let mut bb = Blackboard::default();
    let tree = retry(3, instant("a", false));
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Failure
    );
    assert_eq!(bb.ran.len(), 3);

    let tree = repeat(0, instant("never", true));
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Success
    );
    let tree = retry(0, instant("never", true));
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Failure
    );
    assert!(!bb.ran.contains(&"never"));
}

#[test]
fn retry_stops_at_the_first_success() {
    let tree = retry(
        5,
        leaf(|bb: &mut Blackboard| {
            bb.ran.push("try");
            if bb.ran.len() == 2 {
                NodeResult::<&'static str>::Success
            } else {
                NodeResult::Failure
            }
        }),
    );
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::default();
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Success
    );
    assert_eq!(bb.ran.len(), 2);
}

#[test]
fn if_else_switches_branch_on_evaluate_only() {
    let tree = if_else(|bb: &Blackboard| bb.flag, task("then"), task("else"));
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::default();

    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate).act(),
        Some("else")
    );
    bb.flag = true;
    let mut resumed = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut resumed, &mut bb, EntryMode::Evaluate).act(),
        Some("then")
    );
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate).act(),
        Some("then")
    );
    assert_eq!(*bb.cancelled.lock().unwrap(), ["else"]);
}

#[test]
fn invert_and_force_map_terminal_results_only() {
    fn run(tree: &impl BtNode<Blackboard, &'static str>) -> NodeResult<&'static str> {
        update(
            tree,
            &mut BtState::new(tree),
            &mut Blackboard::default(),
            EntryMode::Evaluate,
        )
    }
    assert_eq!(run(&invert(instant("a", true))), NodeResult::Failure);
    assert_eq!(run(&invert(instant("a", false))), NodeResult::Success);
    assert_eq!(
        run(&force_success(instant("a", false))),
        NodeResult::Success
    );
    assert_eq!(run(&force_failure(instant("a", true))), NodeResult::Failure);

    let tree = force_failure(task("a"));
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::default();
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate).act(),
        Some("a")
    );
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate),
        NodeResult::Failure
    );
}

#[test]
fn reevaluate_when_lets_a_subtree_reconsider_under_resume() {
    let pick = select((
        seq((check(|bb: &Blackboard| bb.flag), task("urgent"))),
        action_while(|_: &Blackboard| true, |_: &Blackboard| "idle"),
    ));
    let tree = reevaluate_when(|bb: &Blackboard| bb.flag, pick);
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::default();

    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume).act(),
        Some("idle")
    );
    bb.flag = true;
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume).act(),
        Some("urgent")
    );
}

#[test]
fn reevaluate_when_passes_resume_through_when_its_condition_is_false() {
    let pick = select((
        seq((check(|bb: &Blackboard| bb.flag), task("urgent"))),
        action_while(|_: &Blackboard| true, |_: &Blackboard| "idle"),
    ));
    let tree = reevaluate_when(|_: &Blackboard| false, pick);
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::default();

    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume).act(),
        Some("idle")
    );
    bb.flag = true;
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume).act(),
        Some("idle")
    );
}
