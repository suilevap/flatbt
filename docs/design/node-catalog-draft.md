# Node catalog: brainstorm and plan

Status: proposal. Nothing here is implemented. Each item says what it is for,
whether the current core can express it, and roughly what it costs.

The catalog lives in `flatbt-nodes` behind one `catalog` feature, per the
[decision log](decisions.md) ("reusable Wait, priority/random selection,
throttling, and decorators belong in a future catalog crate").

## Constraints that shape the catalog

These follow from the current core and decide most of the plan below.

1. **A suspended tree does not re-read what is above the running node.**
   `Resume` goes straight to the active child, `seq` continues its active child
   under `Evaluate`, and a `select` resumes its active branch without re-reading
   that branch's own checks. So "do this while X holds" cannot be written as
   `seq((check(x), action))`. `guard` fills this gap, and it is the catalog's
   answer to a reactive sequence: there will be no reactive sequence control.
2. **`Running` has to carry an act.** A node that yields without a running
   child has nothing to report, so nodes that yield on their own take an act
   closure `Fn(&C) -> A`. `A: Default` is not required, because a default would
   make "busy with nothing" possible again.
3. **Invocation state is dropped on a terminal result.** Anything that must
   outlive one invocation lives explicitly in the blackboard, reached through
   an accessor closure. No core-managed persistent memory.
4. **Abort only runs Drop, and Drop gets no context.** A decorator can abort
   its child by dropping the child's state, which fires `CancelOnDrop`. Hooks
   that need context on abort are not offered.
5. **`BtControl::begin` already gets `&mut C` and the saved active index.**
   That is enough for random policies (RNG in context), utility scoring, and
   inertia. None of them needs a core change.
6. **One active child per control.** `BtChildren` keeps a single variant.
   Parallel execution needs a different children trait.
7. **The policy loop has no iteration budget.** Looping nodes bound
   themselves: at most one restart per update.
8. **No dependencies, and no time in the API.** RNG comes from context. Time
   is a separate milestone (see *Time*).

## Candidates

Priority: **P1** = build first, fixes a real authoring gap. **P2** = common
and cheap. **P3** = useful, but costlier or needs a decision first.
Size: **S** < 100 lines with tests, **M** a few hundred, **L** needs new
traits or generated code.
Core fit: **fits** = public API only. **decision** = fits, but an open
question must be settled first.

### Decorators (wrap one child, implement `BtNode` directly)

| Node | Semantics | Pri | Size | Core fit |
| --- | --- | --- | --- | --- |
| `guard(cond, child)` | Checks `cond` on every update, under both modes and before first entry. False on entry: Failure, and the child never starts. False later: the child's state is dropped (cancellation runs) and the node returns Failure. Otherwise it passes the child's result through. | P1 | S | fits |
| `repeat_while(cond, child)` | Loops the child until `cond` is false, see the case table below. | P1 | S–M | fits |
| `map_act(f, child)` | Turns the child's `NodeResult<B>` into `NodeResult<A>`. Lets a subtree keep its own act type and be reused under a larger one. | P1 | S | fits |
| `invert`, `force_success`, `force_failure` | Standard result mapping. `Running` passes through. | P2 | S | fits |
| `focus(lens, child)` | Runs a subtree over `&mut D` projected from `&mut C`. Lets a subtree be reused across blackboards. | P2 | S | fits (lens closure `for<'a> Fn(&'a mut C) -> &'a mut D`) |
| `reevaluate_when(cond, child)` | Passes `Resume` down as `Evaluate`, but only on updates where `cond(ctx)` is true, for example `\|bb\| bb.alarm_changed`. Never converts blindly. The in-tree counterpart of the Bevy `pace` function, scoped to one subtree. | P2 | S | fits |
| `repeat(n, child)`, `retry(n, child)` | Count successes or failures inside the invocation. `Repeat` in `examples/support` becomes the catalog version. At most one restart per update (constraint 7). | P2 | S | fits |
| `chance(p, rng, child)` | Enters the child with probability `p` on fresh entry only. | P3 | S | fits |

`repeat_while(cond, child)` keeps the agent busy with `child` while `cond`
holds, and succeeds once it no longer does. It reads as a prerequisite for the
next node in a sequence: `seq((repeat_while(far_away, move_closer), interact))`.
An agent that is already close succeeds without moving.

It fails only when `cond` still holds and the child can no longer occupy the
agent. `cond` is checked on entry, on every update, and again whenever the
child completes:

| Situation | Result |
| --- | --- |
| `cond` false: on entry, while the child runs, or after it completes | Success. A running child is aborted; on entry the child never starts. |
| Child succeeds after running, `cond` still true | Restart the child in the same update. |
| Child fails, `cond` still true | Failure. |
| Child completes without returning `Running` in this iteration, `cond` still true | Failure. A loop around an instant child would spin without an act to report (constraints 2 and 7); a restart that completes instantly also logs a diagnostic. |

`guard` is the opposite contract: `cond` is a requirement rather than a goal,
so a false `cond` fails it, and it never restarts its child.

Params: `guard` and `repeat_while` forward scope parameters to the child, and the
condition may read them (`Fn(&C, P) -> bool`), using the same `ParamValue`
reborrow as `ControlNode`. A guard on a scope local, like
`guard(|_, target: &Enemy| target.alive, ..)`, is the common case inside
`scope!`.

### Action and function helpers

Action helpers share the `action_` prefix, next to the existing `action(T)`.
Leaf helpers keep the kind as prefix (`leaf_`, `check_`).

| Helper | Semantics | Pri | Size | Core fit |
| --- | --- | --- | --- | --- |
| `action_while(cond, act)` | The most common action: `Running(act(ctx))` while `cond(ctx)` holds, then Success. Waiting for something is the same node with the condition negated. | P1 | S | fits |
| `action_fn(start, in_progress, tick)` (+ `.complete(..)`) | Builds a `BtAction` from closures, with no struct or impl. Closures receive params, so it works with `.with(local)`. | P1 | S–M | decision (closure inference across the params HRTB; spike first) |
| `leaf_with(f)`, `check_with(f)` | `leaf` and `check` whose closure also receives params (`Fn(&mut C, P)`). Today `leaf` ignores params. Separate constructors, because a second `Leaf` impl would overlap under coherence. | P1 | S | fits |
| `produce(f)` | Synchronous output node: `Fn(&mut C, In) -> Option<T>` writes an `out` local, and `None` means Failure. Generalizes `scope::compute` to producers placed mid-body. | P2 | S | fits |

`leaf_with`, `check_with`, and `produce` are the "sync function for scope"
helpers: they finish within one update and read or write scope locals through
ordinary `.with(..)` bindings, so `scope!` needs no macro changes.

### Control policies (implement `BtControl`)

| Policy | Semantics | Pri | Size | Core fit |
| --- | --- | --- | --- | --- |
| `utility!(\|c: &C\| { score_a => a, score_b => b })` / `utility(scorer, children)` | Runs the child with the highest score. When it fails, rescores and tries the best **untried** child (a `u64` bitmask in policy state). Under `Evaluate` it rescores and may preempt. An optional `inertia` adds a bonus to `active_child_index`, which `begin` already receives, to prevent flip-flopping. Scores are `f32`; NaN counts as "skip". | P1 | M | fits. The macro reuses `__flatbt_child_indices` like `choose!`, and generates `Fn(&C, usize) -> f32`. |
| `priority(...)` | Dynamic priority selector: the same machinery with integer priorities and a stable tie-break on child order. One generic policy (`S: PartialOrd`) with two constructors. | P1 | S (on top of utility) | fits |
| `random_select(rng, children)` | On fresh entry, picks an untried child uniformly at random. On failure, tries another untried child. Under `Evaluate`, keeps the active child, like `seq`. | P2 | S | fits (`rng: Fn(&mut C) -> u32` from context) |
| `weighted_select(rng, weights, children)` | Weighted version. `weights: Fn(&C, usize) -> f32`. | P2 | S | fits |
| `shuffle_seq(rng, children)` | A sequence in random order, without replacement, using the bitmask. | P2 | S | fits |
| `if_else(cond, then, else)` | Sugar over `Choose`. | P2 | S | fits |
| `seq_any` / `try_all` | Runs all children regardless of failures. Success if any succeeded (or if all did, for a variant). | P3 | S | fits |
| `round_robin(memory, children)` | Continues after the child used last time. The last index lives in the blackboard, reached through `memory`. | P3 | S | fits |
| `parallel(policy, (a, b, ..))` | All children run. Policy: all / any / race. The act comes from the first running child, or from a `merge: Fn(A, A) -> A`. | P3 | L | new children trait with generated tuple impls; shares core's tuple generator |

Policy bitmask limit: 64 children. A larger `FLATBT_MAX_CHILDREN` makes
`begin` report `ControlOp::error` instead of misbehaving.

## Time (separate milestone)

Time is not part of the API today, and adding it ad hoc to each node would be
convoluted. Candidate shape: an optional trait the blackboard implements,

```rust,ignore
pub trait BtClock {
    type Instant: Copy + Ord + Send + 'static;
    fn now(&self) -> Self::Instant;
}
```

It would unlock, together:

- `timeout(limit, child)`: abort and fail past `limit`.
- `action_wait(duration, act)`: report `act` until `duration` has passed.
- `cooldown(period, memory, child)`: fail while in cooldown; the last
  completion lives in the blackboard.
- `reevaluate_every(period, child)`: a time-based `reevaluate_when`.

Update counting (`wait_updates`) belongs here too: an update is not a frame
under `Tick::Skip`, so counting updates is a clock in disguise.

Open: how durations are represented without a dependency (an associated
`Duration` type with `Add<Duration, Output = Instant>`?), and whether Bevy's
`Time` can back it directly.

## Core changes

| Change | Unlocks | Recommendation |
| --- | --- | --- |
| Exported tuple generator for "all children alive" | `parallel` | Do it together with `parallel`, in core's `build.rs`. A codegen export, not a runtime change. |
| Context passed to abort | `on_abort` hooks | **Reject.** It contradicts state-owned cancellation. |

No resume hook for control policies (reactive sequence is covered by `guard`)
and no core-managed persistent memory (the blackboard holds it explicitly).

## Plan

Each phase ends with the full check list from `CONTRIBUTING.md`, README entries
for what shipped, and a decision-log entry.

**Phase 1: guards and leaf helpers** (P1, all S)

- `guard`, `repeat_while`, and `map_act`.
- `action_while`, `leaf_with`, and `check_with`.
- Tests pin the key contract: `guard` aborts a suspended action under
  **`Resume`**, and its `CancelOnDrop` fires. That is the case that fails with
  `seq((check, action))` today.

**Phase 2: utility / priority selection** (M)

- A generic scored policy, the `utility!` macro (sharing the child-index
  machinery with `choose!`), `priority`, and inertia.
- Tests: preemption under `Evaluate`, fallback on failure in score order,
  inertia preventing switches, and NaN skipped.

**Phase 3: random policies and remaining decorators** (S each)

- `random_select`, `weighted_select`, and `shuffle_seq`, with an RNG from
  context. Tests use a deterministic counter RNG.
- `invert`, `force_*`, `repeat`/`retry` (moved out of `examples/support`),
  `reevaluate_when`, `focus`, `if_else`.
- `action_fn` after a spike on closure inference; `produce`.

**Phase 4: P3**

- `round_robin`, `seq_any`, `chance`.
- `parallel`, with the shared tuple generator.

**Time milestone** (after Phase 3): `BtClock`, then the nodes it unlocks.

## Deferred

- Scope params for the utility scorer. `BtControl` never receives params, so
  it would need its own node over `BtChildren` plus a `utility { .. }` block in
  `scope!`. Revisit if a real tree needs it; until then the scorer reads the
  blackboard.
