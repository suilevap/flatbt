use std::sync::{Arc, Mutex};

use flatbt::prelude::*;

#[derive(Default)]
struct Ctx {
    ran: Vec<&'static str>,
    fail: Vec<&'static str>,
    flag: bool,
    cancelled: Arc<Mutex<Vec<&'static str>>>,
}

/// Records its name; fails if listed in `fail`, otherwise runs one update and
/// succeeds. Dropped before completing, it records a cancellation.
fn task(name: &'static str) -> impl BtNode<Ctx, &'static str> {
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

impl BtAction<Ctx, &'static str> for Task {
    type State = Running;

    fn start(&self, ctx: &mut Ctx, _: ()) -> Option<Running> {
        ctx.ran.push(self.0);
        (!ctx.fail.contains(&self.0)).then(|| Running {
            name: self.0,
            ticked: false,
            done: false,
            cancelled: ctx.cancelled.clone(),
        })
    }

    fn is_in_progress(&self, state: &Running, _: &Ctx, _: ()) -> bool {
        !state.ticked
    }

    fn tick(&self, state: &mut Running, _: &mut Ctx, _: ()) -> &'static str {
        state.ticked = true;
        self.0
    }

    fn complete(&self, state: &mut Running, _: &mut Ctx, _: ()) -> bool {
        state.done = true;
        true
    }
}

fn instant(name: &'static str, succeeds: bool) -> impl BtNode<Ctx, &'static str> {
    leaf(move |ctx: &mut Ctx| {
        ctx.ran.push(name);
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
    let mut ctx = Ctx::default();
    let mut updates = 0;
    while update(&tree, &mut state, &mut ctx, EntryMode::Resume).is_running() {
        updates += 1;
    }
    assert_eq!((updates, ctx.ran.len()), (3, 3));

    let mut ctx = Ctx::default();
    let tree = retry(3, instant("a", false));
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        NodeResult::Failure
    );
    assert_eq!(ctx.ran.len(), 3);

    let tree = repeat(0, instant("never", true));
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        NodeResult::Success
    );
    let tree = retry(0, instant("never", true));
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        NodeResult::Failure
    );
    assert!(!ctx.ran.contains(&"never"));
}

#[test]
fn retry_stops_at_the_first_success() {
    let tree = retry(
        5,
        leaf(|ctx: &mut Ctx| {
            ctx.ran.push("try");
            if ctx.ran.len() == 2 {
                NodeResult::<&'static str>::Success
            } else {
                NodeResult::Failure
            }
        }),
    );
    let mut state = BtState::new(&tree);
    let mut ctx = Ctx::default();
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        NodeResult::Success
    );
    assert_eq!(ctx.ran.len(), 2);
}

#[test]
fn if_else_switches_branch_on_evaluate_only() {
    let tree = if_else(|ctx: &Ctx| ctx.flag, task("then"), task("else"));
    let mut state = BtState::new(&tree);
    let mut ctx = Ctx::default();

    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate).act(),
        Some("else")
    );
    ctx.flag = true;
    let mut resumed = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut resumed, &mut ctx, EntryMode::Evaluate).act(),
        Some("then")
    );
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate).act(),
        Some("then")
    );
    assert_eq!(*ctx.cancelled.lock().unwrap(), ["else"]);
}

#[test]
fn invert_and_force_map_terminal_results_only() {
    fn run(tree: &impl BtNode<Ctx, &'static str>) -> NodeResult<&'static str> {
        update(
            tree,
            &mut BtState::new(tree),
            &mut Ctx::default(),
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
    let mut ctx = Ctx::default();
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate).act(),
        Some("a")
    );
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate),
        NodeResult::Failure
    );
}

#[test]
fn reevaluate_when_lets_a_subtree_reconsider_under_resume() {
    let pick = select((
        seq((check(|ctx: &Ctx| ctx.flag), task("urgent"))),
        action_while(|_: &Ctx| true, |_: &Ctx| "idle"),
    ));
    let tree = reevaluate_when(|ctx: &Ctx| ctx.flag, pick);
    let mut state = BtState::new(&tree);
    let mut ctx = Ctx::default();

    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume).act(),
        Some("idle")
    );
    ctx.flag = true;
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume).act(),
        Some("urgent")
    );
}

#[test]
fn reevaluate_when_passes_resume_through_when_its_condition_is_false() {
    let pick = select((
        seq((check(|ctx: &Ctx| ctx.flag), task("urgent"))),
        action_while(|_: &Ctx| true, |_: &Ctx| "idle"),
    ));
    let tree = reevaluate_when(|_: &Ctx| false, pick);
    let mut state = BtState::new(&tree);
    let mut ctx = Ctx::default();

    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume).act(),
        Some("idle")
    );
    ctx.flag = true;
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume).act(),
        Some("idle")
    );
}
