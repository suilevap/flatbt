use std::sync::{Arc, Mutex};

use NodeResult::{Failure, Running, Success};
use flatbt::{BtAction, BtState, EntryMode, NodeResult, action, check, leaf, select, seq, update};

type Trace = Arc<Mutex<Vec<String>>>;

#[derive(Default)]
struct Context {
    urgent: bool,
    effects: Vec<&'static str>,
    trace: Trace,
}

struct Task {
    name: &'static str,
    steps: Option<usize>,
    succeeds: bool,
}

// Deliberately has no Default implementation.
struct TaskState {
    name: &'static str,
    remaining: usize,
    completed: bool,
    trace: Trace,
}

impl Drop for TaskState {
    fn drop(&mut self) {
        let mut trace = self.trace.lock().unwrap();
        if !self.completed {
            trace.push(format!("cancel {}", self.name));
        }
        trace.push(format!("drop {}", self.name));
    }
}

impl BtAction<Context> for Task {
    type State = TaskState;

    fn start(&self, ctx: &mut Context, _: ()) -> Option<TaskState> {
        ctx.trace
            .lock()
            .unwrap()
            .push(format!("start {}", self.name));
        Some(TaskState {
            name: self.name,
            remaining: self.steps?,
            completed: false,
            trace: ctx.trace.clone(),
        })
    }

    fn is_in_progress(&self, state: &TaskState, ctx: &Context, _: ()) -> bool {
        ctx.trace
            .lock()
            .unwrap()
            .push(format!("progress {}", self.name));
        state.remaining > 0
    }

    fn complete(&self, state: &mut TaskState, ctx: &mut Context, _: ()) -> bool {
        state.completed = true;
        ctx.trace
            .lock()
            .unwrap()
            .push(format!("complete {}", self.name));
        self.succeeds
    }

    fn tick(&self, state: &mut TaskState, ctx: &mut Context, _: ()) {
        state.remaining -= 1;
        ctx.effects.push(self.name);
        ctx.trace
            .lock()
            .unwrap()
            .push(format!("tick {}", self.name));
    }
}

fn task(name: &'static str, steps: Option<usize>, succeeds: bool) -> impl flatbt::BtNode<Context> {
    action(Task {
        name,
        steps,
        succeeds,
    })
}

#[test]
fn rejected_start_fails_without_progress_completion_or_tick() {
    let root = task("rejected", None, true);
    let mut state = BtState::new(&root);
    let mut ctx = Context::default();
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Resume),
        Failure
    );
    assert_eq!(*ctx.trace.lock().unwrap(), ["start rejected"]);
    assert!(ctx.effects.is_empty());
    assert!(!state.is_running());
}

#[test]
fn initially_finished_action_completes_immediately_with_either_terminal_result() {
    for succeeds in [true, false] {
        let root = task("instant", Some(0), succeeds);
        let mut state = BtState::new(&root);
        let mut ctx = Context::default();
        assert_eq!(
            update(&root, &mut state, &mut ctx, EntryMode::Evaluate),
            if succeeds { Success } else { Failure }
        );
        assert_eq!(
            *ctx.trace.lock().unwrap(),
            [
                "start instant",
                "progress instant",
                "complete instant",
                "drop instant"
            ]
        );
        assert!(ctx.effects.is_empty());
        assert!(!state.is_running());
    }
}

#[test]
fn completion_advances_sequence_and_ticks_next_action_in_the_same_update() {
    let root = seq((
        task("move", Some(1), true),
        leaf(|ctx: &mut Context| {
            ctx.trace.lock().unwrap().push("check".into());
            Success
        }),
        task("fire", Some(1), true),
    ));
    let mut state = BtState::new(&root);
    let mut ctx = Context::default();
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    assert_eq!(ctx.effects, ["move"]);
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    assert_eq!(ctx.effects, ["move", "fire"]);
    assert_eq!(
        *ctx.trace.lock().unwrap(),
        [
            "start move",
            "progress move",
            "tick move",
            "progress move",
            "complete move",
            "drop move",
            "check",
            "start fire",
            "progress fire",
            "tick fire",
        ]
    );
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Resume),
        Success
    );
    assert_eq!(ctx.effects, ["move", "fire"]);
    assert!(!state.is_running());
}

#[test]
fn speculative_completion_loses_or_new_action_preempts_without_ticking_old_action() {
    for candidate_steps in [0, 1] {
        let root = select((
            seq((
                check(|ctx: &Context| ctx.urgent),
                task("attack", Some(candidate_steps), false),
            )),
            task("move", Some(3), true),
        ));
        let mut state = BtState::new(&root);
        let mut ctx = Context::default();
        assert_eq!(
            update(&root, &mut state, &mut ctx, EntryMode::Resume),
            Running
        );
        ctx.trace.lock().unwrap().clear();
        ctx.urgent = true;
        assert_eq!(
            update(&root, &mut state, &mut ctx, EntryMode::Evaluate),
            Running
        );
        if candidate_steps == 0 {
            assert_eq!(ctx.effects, ["move", "move"]);
            assert_eq!(
                *ctx.trace.lock().unwrap(),
                [
                    "start attack",
                    "progress attack",
                    "complete attack",
                    "drop attack",
                    "progress move",
                    "tick move",
                ]
            );
        } else {
            assert_eq!(ctx.effects, ["move", "attack"]);
            assert_eq!(
                *ctx.trace.lock().unwrap(),
                [
                    "start attack",
                    "progress attack",
                    "tick attack",
                    "cancel move",
                    "drop move",
                ]
            );
        }
        state.reset();
        let trace = ctx.trace.lock().unwrap();
        assert_eq!(trace.iter().filter(|e| *e == "drop move").count(), 1);
        assert_eq!(trace.iter().filter(|e| *e == "drop attack").count(), 1);
    }
}

// An enclosing node can reject a child after the child's inline tick has run.
struct Reject<N>(N);

impl<C, N: flatbt::BtNode<C>> flatbt::BtNode<C> for Reject<N> {
    type State = N::State;

    fn update(&self, state: &mut Self::State, ctx: &mut C, _: (), mode: EntryMode) -> NodeResult {
        let _ = self.0.update(state, ctx, (), mode);
        Failure
    }
}

#[test]
fn rejecting_a_running_candidate_does_not_undo_its_inline_tick() {
    let root = select((
        Reject(task("candidate", Some(2), true)),
        leaf(|_: &mut Context| Running),
    ));
    let mut state = BtState::new(&root);
    let mut ctx = Context::default();
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Evaluate),
        Running
    );
    assert_eq!(ctx.effects, ["candidate"]);
    assert_eq!(
        *ctx.trace.lock().unwrap(),
        [
            "start candidate",
            "progress candidate",
            "tick candidate",
            "cancel candidate",
            "drop candidate",
        ]
    );
    assert!(state.is_running());
}

#[test]
fn reset_and_drop_cancel_uncompleted_state_once_without_extra_updates() {
    let root = seq((task("work", Some(1), true),));
    let mut state = BtState::new(&root);
    let mut ctx = Context::default();
    state.reset();
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    state.reset();
    state.reset();
    assert!(!state.is_running());
    // Last tick finished the work, but complete has not observed it yet.
    assert_eq!(
        *ctx.trace.lock().unwrap(),
        [
            "start work",
            "progress work",
            "tick work",
            "cancel work",
            "drop work",
        ]
    );
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    drop(state);
    let trace = ctx.trace.lock().unwrap();
    assert_eq!(trace.iter().filter(|e| *e == "cancel work").count(), 2);
    assert_eq!(trace.iter().filter(|e| *e == "drop work").count(), 2);
}

#[test]
fn terminal_revalidation_cancels_the_old_running_branch() {
    let root = select((
        check(|ctx: &Context| ctx.urgent),
        task("move", Some(3), true),
    ));
    let mut state = BtState::new(&root);
    let mut ctx = Context::default();
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    ctx.trace.lock().unwrap().clear();
    ctx.urgent = true;
    assert_eq!(
        update(&root, &mut state, &mut ctx, EntryMode::Evaluate),
        Success
    );
    assert_eq!(*ctx.trace.lock().unwrap(), ["cancel move", "drop move"]);
    assert!(!state.is_running());
}
