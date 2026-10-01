# Goal stack: goals that need other goals

Status: first implementation in `flatbt::nodes::goal`; API open to change.

## Problem

Agents often solve a goal by first solving whatever blocks it: reach a room,
blocked by a door, which needs a key, which is in another room. A static tree
can spell out a chain it knows in advance, but not one whose links are data
("door 7 needs key 3"), and a subtree cannot contain itself. GOAP-style search
solves it, at the cost of a planner, a world model and allocation.

## Design

A goal is a value. One dispatch subtree has an arm per kind of goal, and a goal
asks for what it needs first by calling that dispatch again for a subgoal.

```rust
goals::<8, _, _>(
    |w: &World| Goal::Reach(w.target),            // the root goal, read every update
    goal_match!(|goal: &Goal| {
        Goal::Reach(_) => select((
            seq((need(door_in_the_way), with_goal(leaf_with(walk)))),
            seq((need(climb_if_blocked), with_goal(leaf_with(walk)))),
        )),
        Goal::OpenDoor => seq((need(key_if_missing), leaf(open))),
        Goal::GetKey => seq((need(reach_key), leaf(pick_up))),
        Goal::Climb => action(Climb),
    }),
)
```

### `need` is a call, not a push

The first sketch had `need` push a subgoal and a driver pop it on success. A
push has no honest result: the subtree has to stop, but `Failure` would let a
`select` run its next way in the same update and `Success` would let a `seq`
act. So `need(g)` runs the dispatch for `g` one frame deeper and returns its
result, as a function call does:

| Subgoal result | `need` returns | So |
| --- | --- | --- |
| `Running(act)` | `Running(act)` | The agent works on the subgoal; the act is the subgoal's. |
| `Success` | `Success` | `seq((need(..), act))` acts once the blocker is gone. |
| `Failure` | `Failure` | `select((need(a), need(b)))` tries the next way. |
| no subgoal (`None`) | `Success` | Nothing is in the way. |

Push and pop fall out: a frame starts when `need` asks for a new goal and ends
when the goal completes, or when an update no longer reaches the `need` that
asked for it, which is how a preempted branch ends. No node pushes or pops.

### Failed subgoals

A subgoal that fails is recorded against the goal that asked for it, for as
long as that goal stays on the stack. `need` fails at once for a recorded goal,
so `Evaluate` rescanning a `select` falls through to the next way instead of
retrying the one that failed. The record goes with its asker. The list is
fixed-size; when full, a failure is not recorded and a diagnostic is logged,
and the stack depth still bounds the search.

`need` also fails without running when its goal is already on the stack (a
cycle: the key is behind the door it opens) and when the stack is full.

### Reactivity

Every update runs from the root down, `Resume` included, so each `need`
re-asks for its subgoal. A goal achieved by other means makes its `need`
return `None`, and the frames below it end that update. A changed root goal
starts over.

### Storage

Run state: the goals, one dispatch run state per frame, and the failure list,
`N` of each, so the run state is `N` times the dispatch subtree's. No heap.
Memory: one dispatch memory per stack depth, since memory cannot be shared by
two frames running at once. A cooldown in a goal's subtree is therefore per
depth, not per goal. The stack is run state, so preemption drops it; a goal
that must survive belongs in the blackboard, where `root` reads it.

### Parameters

The dispatch subtree receives `GoalCall`: the goal and the way to subgoals.
Catalog nodes pass it through. `with_goal(node)` gives a node `&Goal`;
`no_params(node)` a node taking `()`, such as an `action` of a
`BtAction<C, A>`.

## Open

- Traces stop at `need`: it records the subgoal and its result, but the
  subgoal's nodes would reuse the dispatch's ids. Numbering frames separately
  would make them traceable.
- `goal_match!` patterns cannot bind into the subtree; closures read the goal.
  Binding them as `scope!` locals would remove the `let Goal::Reach(to) = goal`
  boilerplate.
- `need` as the name; `require`, `achieve` and `subgoal` are candidates.
- Per-goal cooldowns and backoff (memory keyed by goal rather than node).
