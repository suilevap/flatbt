# Goal stack: goals that need other goals

Status: first implementation in `flatbt::nodes::goal`; API open to change.

## Problem

Agents often solve a goal by first solving whatever blocks it: reach a room,
blocked by a door, which needs a key, which is in another room. A static tree
can spell out a chain it knows in advance, but not one whose links are data
("door 7 needs key 3"), and a subtree cannot contain itself. GOAP-style search
solves it, at the cost of a planner, a world model and allocation.

## Design

A goal is a value. One dispatch subtree has an arm per kind of goal. `goals`
keeps a stack of goals and runs the dispatch for the one on top: a sequence
whose next step is chosen by the current goal, and which returns to the
previous goal with a result.

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
.done(|w: &World, goal: &Goal| /* already achieved? */)
```

### One goal runs at a time

Each update runs only the top goal's subtree. No recursion: one loop in
`goals`, bounded by the stack.

| Top goal's subtree | Then |
| --- | --- |
| `Running(act)` | The update returns it. |
| Waiting at a `need` that pushed `g` | `g` goes on top and runs, in the same update. The waiting goal keeps its run state. |
| `Success` / `Failure` | The goal is popped and its result kept for the goal below, which resumes at its `need` in the same update. Popping the root ends the node with that result. |

`need(f)` asks `f(ctx, goal)` for a subgoal:

| Answer | `need` |
| --- | --- |
| `None` | Succeeds: nothing in the way. |
| `Some(g)`, already ended for this goal | Returns that result. |
| `Some(g)`, on the stack | Fails: a cycle (the key is behind the door it opens). |
| `Some(g)`, new | Pushes `g` and waits: `Running(A::default())`. |

Waiting is `Running`, so a `select` or `seq` stops at the `need` as it would at
any running child, and the goal resumes there with `Resume` when `g` returns,
its sequence progress intact. The act inside is a placeholder: `goals` sees the
push and runs `g` in the same update, so it never leaves the stack. `need`
therefore needs `A: Default`.

Each goal asks for a given subgoal at most once while it is on the stack, so a
failed way is not retried when `Evaluate` rescans it, and `select` falls
through to the next. The results go when their asker is popped. This also
bounds the work per update; a backstop logs a diagnostic and fails the node if
it does not settle.

### Reactivity

Goals below the top do not run while it works. `goals(..).done(|ctx, goal|
..)` is asked for every goal on the stack on every update, from the root up;
the first one already achieved is popped with everything above it, as if it
had succeeded. A changed root goal starts over. The stack is run state, so
preemption drops it; a goal that must survive belongs in the blackboard, where
`root` reads it.

### Storage

Run state: `N` goals, each with a run state of the dispatch subtree, and their
results; so `N` times the dispatch subtree's run state. Memory: the dispatch
subtree's, shared by every goal, as only one runs at a time. No heap.

### Parameters

The dispatch subtree receives `GoalCall`: the goal and the stack. Catalog
nodes pass it through. `with_goal(node)` gives a node `&Goal`;
`no_params(node)` a node taking `()`, such as an `action` of a
`BtAction<C, A>`.

### Rejected

- **`need` as a call**: running the subgoal from inside `need`, every update
  walking from the root goal down. Reactive without `done`, but recursive,
  with a memory per stack depth, and traces stopped at `need`.
- **`need` failing to end the turn**, the parent re-running from its start on
  return: a `select` ran the nodes after the `need` in the same turn, and the
  parent lost its progress.

## Open

- A `need` inside a `scope!` does not see the goal: a scope gives its children
  its locals, not its own parameters.
- The placeholder act needs `A: Default`; an act type without one cannot use
  `need`.
- `goal_match!` patterns cannot bind into the subtree; closures read the goal.
- `need` as the name; `require`, `achieve` and `subgoal` are candidates.
