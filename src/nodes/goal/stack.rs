use core::fmt;
use core::marker::PhantomData;

use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult};

/// The parameters of a goal's subtree: the goal, how its subgoals ended, and
/// the request for a new one. The stack itself belongs to [`goals`].
pub struct GoalCall<'a, Goal> {
    /// The goal this subtree works toward.
    pub goal: &'a Goal,
    asker: u8,
    results: &'a [Option<Outcome<Goal>>],
    request: &'a mut Option<Goal>,
}

/// How a subgoal ended, for the goal that asked for it.
struct Outcome<Goal> {
    asker: u8,
    goal: Goal,
    succeeded: bool,
}

impl<Goal: PartialEq> GoalCall<'_, Goal> {
    /// How `goal` ended as a subgoal of this goal: `Some(true)` achieved,
    /// `Some(false)` failed or refused, `None` not asked for yet. Kept while
    /// this goal is on the stack.
    pub fn result(&self, goal: &Goal) -> Option<bool> {
        self.results
            .iter()
            .flatten()
            .find(|outcome| outcome.asker == self.asker && outcome.goal == *goal)
            .map(|outcome| outcome.succeeded)
    }

    /// Asks [`goals`] for `goal` as a subgoal. A node that asks then returns
    /// `Running`: [`goals`] decides whether to push it, and this goal runs
    /// again from its start when it has an answer. The last request in a run
    /// wins.
    pub fn request(&mut self, goal: Goal) {
        *self.request = Some(goal);
    }
}

/// The [`ParamShape`] of [`GoalCall`].
pub struct GoalShape<Goal>(PhantomData<fn() -> Goal>);

impl<Goal: 'static> ParamShape for GoalShape<Goal> {
    type Value<'a> = GoalCall<'a, Goal>;

    fn reborrow<'a, 'b: 'a>(value: &'a mut GoalCall<'b, Goal>) -> GoalCall<'a, Goal> {
        GoalCall {
            goal: value.goal,
            asker: value.asker,
            results: value.results,
            request: &mut *value.request,
        }
    }
}

impl<Goal: 'static> ParamValue for GoalCall<'_, Goal> {
    type Shape = GoalShape<Goal>;

    fn into_value<'a>(self) -> GoalCall<'a, Goal>
    where
        Self: 'a,
    {
        self
    }
}

/// A goal stack over a dispatch subtree.
pub struct Goals<Root, Dispatch, Done, const DEPTH: usize> {
    root: Root,
    dispatch: Dispatch,
    done: Done,
}

/// Runs `dispatch` for the goal on top of a stack of at most `DEPTH` goals,
/// starting with the one `root` reads from the context.
///
/// Only the top goal's subtree runs, and only it has run state. When it asks
/// for a subgoal with [`need`](super::need), its run ends and `goals` pushes
/// the subgoal, which runs in the same update. A subgoal already on the stack
/// (a cycle), or one that would overflow it, is refused instead: the asking
/// goal runs again and is told it failed. When the top goal's subtree succeeds
/// or fails, the goal is popped and the goal below runs again from its start,
/// where its `need` returns that result. The node ends with the root goal.
///
/// `root` is read on every update; when it changes, the stack starts over.
/// Goals below the top do not run while it works, so a goal achieved by other
/// means is noticed only through [`Goals::done`].
pub fn goals<const DEPTH: usize, Root, Dispatch>(
    root: Root,
    dispatch: Dispatch,
) -> Goals<Root, Dispatch, NotDone, DEPTH> {
    const {
        assert!(
            DEPTH > 0 && DEPTH <= u8::MAX as usize,
            "goals needs 1 to 255 frames"
        )
    };
    Goals {
        root,
        dispatch,
        done: NotDone,
    }
}

impl<Root, Dispatch, Done, const DEPTH: usize> Goals<Root, Dispatch, Done, DEPTH> {
    /// Asks `done(ctx, goal)` for every goal on the stack on every update,
    /// from the root up. The first goal already achieved is popped with the
    /// goals above it, as if its subtree had succeeded.
    pub fn done<IsDone>(self, done: IsDone) -> Goals<Root, Dispatch, IsDone, DEPTH> {
        Goals {
            root: self.root,
            dispatch: self.dispatch,
            done,
        }
    }
}

/// No [`Goals::done`] check.
pub struct NotDone;

/// Whether a goal is already achieved.
pub trait GoalDone<Context, Goal> {
    fn is_done(&self, ctx: &Context, goal: &Goal) -> bool;
}

impl<Context, Goal> GoalDone<Context, Goal> for NotDone {
    #[inline(always)]
    fn is_done(&self, _: &Context, _: &Goal) -> bool {
        false
    }
}

impl<Context, Goal, IsDone: Fn(&Context, &Goal) -> bool> GoalDone<Context, Goal> for IsDone {
    #[inline(always)]
    fn is_done(&self, ctx: &Context, goal: &Goal) -> bool {
        self(ctx, goal)
    }
}

/// The stack, the results subgoals returned, and the top goal's run state.
pub struct GoalsState<Goal, DispatchState, const DEPTH: usize> {
    top: DispatchState,
    goals: [Option<Goal>; DEPTH],
    results: [Option<Outcome<Goal>>; DEPTH],
}

impl<Goal, DispatchState: Default, const DEPTH: usize> Default
    for GoalsState<Goal, DispatchState, DEPTH>
{
    fn default() -> Self {
        Self {
            top: DispatchState::default(),
            goals: core::array::from_fn(|_| None),
            results: core::array::from_fn(|_| None),
        }
    }
}

impl<Goal, DispatchState: Default, const DEPTH: usize> GoalsState<Goal, DispatchState, DEPTH> {
    fn depth(&self) -> usize {
        self.goals.iter().take_while(|goal| goal.is_some()).count()
    }

    /// Pops goals down to `depth` and forgets what they were told. The top
    /// goal's run ends.
    fn truncate(&mut self, depth: usize) {
        for goal in &mut self.goals[depth..] {
            *goal = None;
        }
        self.top = DispatchState::default();
        for result in &mut self.results {
            if result
                .as_ref()
                .is_some_and(|outcome| outcome.asker as usize >= depth)
            {
                *result = None;
            }
        }
    }

    /// Pops the top goal and tells the goal below how it ended. `false` when
    /// it was the root.
    fn pop(&mut self, succeeded: bool) -> bool {
        let depth = self.depth();
        let goal = self.goals[depth - 1].take();
        self.truncate(depth - 1);
        let (Some(goal), Some(asker)) = (goal, depth.checked_sub(2)) else {
            return false;
        };
        self.record(asker, goal, succeeded);
        true
    }

    /// Keeps how `goal` ended for the goal at `asker`.
    fn record(&mut self, asker: usize, goal: Goal, succeeded: bool) {
        match self.results.iter_mut().find(|result| result.is_none()) {
            Some(slot) => {
                *slot = Some(Outcome {
                    asker: asker as u8,
                    goal,
                    succeeded,
                })
            }
            // Full: the goal may be asked for again; the loop's backstop
            // ends that.
            None => crate::log_error(format_args!("goal result list is full")),
        }
    }
}

#[cold]
#[inline(never)]
fn too_many_steps<Act>(entry: Entry<'_>) -> NodeResult<Act> {
    entry.error("goals did not settle within one update")
}

impl<
    Context,
    Act,
    Params,
    Goal,
    Root,
    Dispatch,
    Done,
    DispatchState,
    DispatchMemory,
    const DEPTH: usize,
> BtNode<Context, Act, Params> for Goals<Root, Dispatch, Done, DEPTH>
where
    Goal: Clone + PartialEq + fmt::Debug + Send + 'static,
    Root: Fn(&Context) -> Goal,
    Dispatch: for<'a> BtNode<
            Context,
            Act,
            GoalCall<'a, Goal>,
            State = DispatchState,
            Memory = DispatchMemory,
        >,
    Done: GoalDone<Context, Goal>,
    DispatchState: Default + Send + 'static,
    DispatchMemory: Default + Send + 'static,
{
    type State = GoalsState<Goal, DispatchState, DEPTH>;
    type Memory = DispatchMemory;
    const NODES: usize = 1 + <Dispatch as BtNode<Context, Act, GoalCall<'static, Goal>>>::NODES;

    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut DispatchMemory,
        ctx: &mut Context,
        _: Params,
        entry: Entry<'_>,
    ) -> NodeResult<Act> {
        // Whether the top goal's run starts now: pushed, or back from a
        // subgoal. Otherwise it continues as the update entered `goals`.
        let mut fresh = false;
        let root = (self.root)(ctx);
        if state.goals[0].as_ref() != Some(&root) {
            state.truncate(0);
            state.goals[0] = Some(root);
            fresh = true;
        }
        if let Some(done) = state
            .goals
            .iter()
            .flatten()
            .position(|goal| self.done.is_done(ctx, goal))
        {
            entry.record("done", || done);
            if done == 0 {
                state.truncate(0);
                return NodeResult::Success;
            }
            // As if the goal had succeeded: popped, and its asker told so.
            state.truncate(done + 1);
            state.pop(true);
            fresh = true;
        }
        // Each step pushes a new goal or pops one. Pushes are bounded by the
        // stack and by each asker's results; this is a backstop.
        for _ in 0..4 * DEPTH * DEPTH + 4 {
            let depth = state.depth();
            let mut request = None;
            let call = GoalCall {
                goal: state.goals[depth - 1].as_ref().expect("depth counts goals"),
                asker: (depth - 1) as u8,
                results: &state.results,
                request: &mut request,
            };
            let top = &mut state.top;
            let result = if fresh {
                entry.run_candidate(1, &self.dispatch, top, memory, ctx, call)
            } else {
                entry.run(1, &self.dispatch, top, memory, ctx, call)
            };
            if let Some(subgoal) = request {
                // Stopped to ask; the act it returned is a placeholder. Its run
                // ends: it starts over when it has an answer.
                state.top = DispatchState::default();
                fresh = true;
                if state.goals.contains(&Some(subgoal.clone())) {
                    // A cycle: the key behind the door it opens.
                    entry.record("cycle", || subgoal.clone());
                    state.record(depth - 1, subgoal, false);
                } else if depth == DEPTH {
                    crate::log_error(format_args!("goal stack is full"));
                    state.record(depth - 1, subgoal, false);
                } else {
                    state.goals[depth] = Some(subgoal);
                }
                continue;
            }
            match result {
                NodeResult::Running(act) => return NodeResult::Running(act),
                NodeResult::Success | NodeResult::Failure => {
                    let succeeded = matches!(result, NodeResult::Success);
                    if !state.pop(succeeded) {
                        return result;
                    }
                    fresh = true;
                }
            }
        }
        state.truncate(0);
        too_many_steps(entry)
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &DispatchMemory,
        inspector: &mut dyn Inspector,
    ) {
        inspector.node(NodeInfo::new("goals", state.is_some()), |inspector| {
            let depth = state.map_or(0, GoalsState::depth);
            let state = state.filter(|_| depth > 0);
            if let Some(state) = state {
                inspector.field("stack", &Stack(&state.goals));
                if state
                    .results
                    .iter()
                    .flatten()
                    .any(|outcome| !outcome.succeeded)
                {
                    inspector.field("failed", &Failed(&state.results));
                }
            }
            BtNode::<Context, Act, GoalCall<'_, Goal>>::inspect(
                &self.dispatch,
                state.map(|state| &state.top),
                memory,
                inspector,
            );
        });
    }
}

struct Stack<'a, Goal>(&'a [Option<Goal>]);

impl<Goal: fmt::Debug> fmt::Debug for Stack<'_, Goal> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.iter().flatten()).finish()
    }
}

struct Failed<'a, Goal>(&'a [Option<Outcome<Goal>>]);

impl<Goal: fmt::Debug> fmt::Debug for Failed<'_, Goal> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(
                self.0
                    .iter()
                    .flatten()
                    .filter(|outcome| !outcome.succeeded)
                    .map(|outcome| &outcome.goal),
            )
            .finish()
    }
}
