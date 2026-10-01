use core::fmt;
use core::marker::PhantomData;

use crate::inspect::{Inspector, NodeInfo};
use crate::params::{ParamShape, ParamValue};
use crate::{BtNode, Entry, NodeResult};

/// Runs subgoals for [`need`](super::need), one stack frame deeper.
pub trait Subgoals<C, A, G> {
    /// Runs the dispatch subtree for `goal` in the next frame and returns its
    /// result. Fails without running it when `goal` is already on the stack,
    /// failed earlier for the goal asking, or the stack is full.
    fn run(&mut self, ctx: &mut C, goal: G, entry: Entry<'_>) -> NodeResult<A>;
}

/// The parameters of a goal's subtree: the goal, and the way to its subgoals.
pub struct GoalCall<'a, C, A, G> {
    /// The goal this subtree works toward.
    pub goal: &'a G,
    subgoals: &'a mut dyn Subgoals<C, A, G>,
}

impl<'a, C, A, G> GoalCall<'a, C, A, G> {
    /// Runs `goal` as a subgoal of this one. See [`Subgoals::run`].
    pub fn need(&mut self, ctx: &mut C, goal: G, entry: Entry<'_>) -> NodeResult<A> {
        self.subgoals.run(ctx, goal, entry)
    }
}

/// The [`ParamShape`] of [`GoalCall`].
pub struct GoalShape<C, A, G>(PhantomData<Types<C, A, G>>);

/// Names the shape's types without owning them.
type Types<C, A, G> = fn() -> (C, A, G);

impl<C: 'static, A: 'static, G: 'static> ParamShape for GoalShape<C, A, G> {
    type Value<'a> = GoalCall<'a, C, A, G>;

    fn reborrow<'a, 'b: 'a>(value: &'a mut GoalCall<'b, C, A, G>) -> GoalCall<'a, C, A, G> {
        GoalCall {
            goal: value.goal,
            subgoals: &mut *value.subgoals,
        }
    }
}

impl<C: 'static, A: 'static, G: 'static> ParamValue for GoalCall<'_, C, A, G> {
    type Shape = GoalShape<C, A, G>;

    fn into_value<'a>(self) -> GoalCall<'a, C, A, G>
    where
        Self: 'a,
    {
        // Rebuilt rather than returned: the coercion shortens the trait
        // object's lifetime, which `&mut` alone keeps invariant.
        GoalCall {
            goal: self.goal,
            subgoals: self.subgoals,
        }
    }
}

/// A goal stack over a dispatch subtree.
pub struct Goals<R, D, const N: usize> {
    root: R,
    dispatch: D,
}

/// Runs `dispatch` for the goal `root` reads from the context, with a stack of
/// at most `N` goals for the subgoals it [needs](super::need).
///
/// `root` is read on every update. When it changes, the stack starts over
/// from the new goal. The node succeeds or fails with the root goal's subtree.
///
/// Memory inside `dispatch` is kept per stack depth: a cooldown in a goal's
/// subtree is shared by every goal run at that depth.
pub fn goals<const N: usize, R, D>(root: R, dispatch: D) -> Goals<R, D, N> {
    const {
        assert!(
            N > 0 && N <= u8::MAX as usize,
            "goals needs 1 to 255 frames"
        )
    };
    Goals { root, dispatch }
}

/// The frames: each one's goal and its subtree's run state, and the subgoals
/// that failed for each goal still on the stack.
pub struct GoalsState<G, S, const N: usize> {
    // Drop subtrees first, deepest last, as the rest of the tree does.
    states: [S; N],
    goals: [Option<G>; N],
    /// `(asker, goal)`: `goal` failed as a subgoal of frame `asker`.
    failed: [Option<(u8, G)>; N],
}

impl<G, S: Default, const N: usize> Default for GoalsState<G, S, N> {
    fn default() -> Self {
        Self {
            states: core::array::from_fn(|_| S::default()),
            goals: core::array::from_fn(|_| None),
            failed: core::array::from_fn(|_| None),
        }
    }
}

/// The dispatch subtree's memory, per stack depth.
pub struct GoalsMemory<M, const N: usize>([M; N]);

impl<M: Default, const N: usize> Default for GoalsMemory<M, N> {
    fn default() -> Self {
        Self(core::array::from_fn(|_| M::default()))
    }
}

/// The goals of the frames above, for the cycle check.
struct Chain<'a, G> {
    goal: &'a G,
    parent: Option<&'a Chain<'a, G>>,
}

impl<G: PartialEq> Chain<'_, G> {
    fn contains(&self, goal: &G) -> bool {
        self.goal == goal || self.parent.is_some_and(|parent| parent.contains(goal))
    }
}

/// The frames from `next` down, for running the goal at `next`.
struct Frames<'a, D, G, S, M> {
    dispatch: &'a D,
    /// Index of the frame this level fills.
    next: usize,
    above: Option<&'a Chain<'a, G>>,
    goals: &'a mut [Option<G>],
    states: &'a mut [S],
    memories: &'a mut [M],
    failed: &'a mut [Option<(u8, G)>],
    /// The deepest frame this update reached.
    deepest: &'a mut usize,
}

/// Ends the frames from `goals[0]` down.
fn clear<G, S: Default>(goals: &mut [Option<G>], states: &mut [S]) {
    for (goal, state) in goals.iter_mut().zip(states.iter_mut()) {
        if goal.take().is_none() {
            break;
        }
        *state = S::default();
    }
}

/// Forgets the failures recorded for frames from `depth` down.
fn forget_failures<G>(failed: &mut [Option<(u8, G)>], depth: usize) {
    for entry in failed.iter_mut() {
        if entry
            .as_ref()
            .is_some_and(|(asker, _)| *asker as usize >= depth)
        {
            *entry = None;
        }
    }
}

#[cold]
#[inline(never)]
fn full<A>(entry: Entry<'_>) -> NodeResult<A> {
    entry.error("goal stack is full")
}

impl<C, A, G, D, S, M> Subgoals<C, A, G> for Frames<'_, D, G, S, M>
where
    C: 'static,
    A: 'static,
    G: Clone + PartialEq + fmt::Debug + Send + 'static,
    D: for<'a> BtNode<C, A, GoalCall<'a, C, A, G>, State = S, Memory = M>,
    S: Default + Send + 'static,
    M: Default + Send + 'static,
{
    fn run(&mut self, ctx: &mut C, goal: G, entry: Entry<'_>) -> NodeResult<A> {
        if self.above.is_some_and(|above| above.contains(&goal)) {
            entry.record("cycle", || goal.clone());
            return NodeResult::Failure;
        }
        let asker = self.next.wrapping_sub(1) as u8;
        if self.next > 0
            && self
                .failed
                .iter()
                .flatten()
                .any(|(by, failed)| *by == asker && *failed == goal)
        {
            entry.record("failed_before", || goal.clone());
            return NodeResult::Failure;
        }
        let (Some(slot), Some(_)) = (self.goals.first(), self.states.first()) else {
            return full(entry);
        };
        let fresh = slot.as_ref() != Some(&goal);
        if fresh {
            clear(self.goals, self.states);
            forget_failures(self.failed, self.next);
            self.goals[0] = Some(goal.clone());
        }
        *self.deepest = (*self.deepest).max(self.next);

        let (slot, goals) = self.goals.split_first_mut().expect("checked above");
        let (state, states) = self.states.split_first_mut().expect("checked above");
        let (memory, memories) = self.memories.split_first_mut().expect("sized as states");
        let goal_ref = slot.as_ref().expect("set above");
        let chain = Chain {
            goal: goal_ref,
            parent: self.above,
        };
        let mut deeper = Frames {
            dispatch: self.dispatch,
            next: self.next + 1,
            above: Some(&chain),
            goals,
            states,
            memories,
            failed: &mut *self.failed,
            deepest: &mut *self.deepest,
        };
        let call = GoalCall {
            goal: goal_ref,
            subgoals: &mut deeper,
        };
        let result = if fresh {
            entry.run_candidate(1, self.dispatch, state, memory, ctx, call)
        } else {
            entry.run(1, self.dispatch, state, memory, ctx, call)
        };
        if !result.is_running() {
            clear(self.goals, self.states);
            forget_failures(self.failed, self.next);
            if matches!(result, NodeResult::Failure) && self.next > 0 {
                match self.failed.iter_mut().find(|entry| entry.is_none()) {
                    Some(entry) => *entry = Some((asker, goal)),
                    // Full: the goal may be asked for again; the stack still
                    // bounds how deep that goes.
                    None => crate::log_error(format_args!("goal failure list is full")),
                }
            }
        }
        result
    }
}

impl<C, A, P, G, R, D, S, M, const N: usize> BtNode<C, A, P> for Goals<R, D, N>
where
    C: 'static,
    A: 'static,
    G: Clone + PartialEq + fmt::Debug + Send + 'static,
    R: Fn(&C) -> G,
    D: for<'a> BtNode<C, A, GoalCall<'a, C, A, G>, State = S, Memory = M>,
    S: Default + Send + 'static,
    M: Default + Send + 'static,
{
    type State = GoalsState<G, S, N>;
    type Memory = GoalsMemory<M, N>;
    const NODES: usize = 1 + <D as BtNode<C, A, GoalCall<'static, C, A, G>>>::NODES;

    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,
        ctx: &mut C,
        _: P,
        entry: Entry<'_>,
    ) -> NodeResult<A> {
        let root = (self.root)(ctx);
        let mut deepest = 0;
        let mut frames = Frames {
            dispatch: &self.dispatch,
            next: 0,
            above: None,
            goals: &mut state.goals,
            states: &mut state.states,
            memories: &mut memory.0,
            failed: &mut state.failed,
            deepest: &mut deepest,
        };
        // The root goal's subtree is traced as this node's child; subgoals are
        // not traced below the `need` that runs them.
        let result = frames.run(ctx, root, entry);
        // Frames below the deepest one reached were left this update, as a
        // preempted branch is: end them.
        if result.is_running() {
            clear(
                &mut state.goals[deepest + 1..],
                &mut state.states[deepest + 1..],
            );
            forget_failures(&mut state.failed, deepest + 1);
        }
        result
    }

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &Self::Memory,
        inspector: &mut dyn Inspector,
    ) {
        inspector.node(NodeInfo::new("goals", state.is_some()), |inspector| {
            let depth = state.map_or(0, |state| {
                state.goals.iter().take_while(|goal| goal.is_some()).count()
            });
            if let Some(state) = state.filter(|_| depth > 0) {
                inspector.field("stack", &Stack(&state.goals[..depth]));
                if state.failed.iter().any(Option::is_some) {
                    inspector.field("failed", &Failed(&state.failed));
                }
            }
            // The subtree of the goal on top of the stack.
            let top = depth.saturating_sub(1);
            BtNode::<C, A, GoalCall<'_, C, A, G>>::inspect(
                &self.dispatch,
                state.filter(|_| depth > 0).map(|state| &state.states[top]),
                &memory.0[top],
                inspector,
            );
        });
    }
}

struct Stack<'a, G>(&'a [Option<G>]);

impl<G: fmt::Debug> fmt::Debug for Stack<'_, G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.iter().flatten()).finish()
    }
}

struct Failed<'a, G>(&'a [Option<(u8, G)>]);

impl<G: fmt::Debug> fmt::Debug for Failed<'_, G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().flatten().map(|(_, goal)| goal))
            .finish()
    }
}
