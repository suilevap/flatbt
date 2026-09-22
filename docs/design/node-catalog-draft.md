# Node catalog: brainstorm and plan

Status: proposal. Nothing here is implemented. Each item says what it is for,
whether the current core can express it, and roughly what it costs.

The catalog belongs in `flatbt-nodes` behind features, per the
[decision log](decisions.md) ("reusable Wait, priority/random selection,
throttling, and decorators belong in a future catalog crate").

## Constraints that shape the catalog

These follow from the current core and decide most of the plan below.

1. **A suspended tree does not re-read what is above the running node.**
   `Resume` goes straight to the active child, `seq` continues its active child
   under `Evaluate`, and a `select` resumes its active branch without re-reading
   that branch's own checks. So "do this while X holds" cannot be written as
   `seq((check(x), action))`. Today the answer is "put it in `is_in_progress`",
   which means every action repeats its own conditions. A decorator that
   re-reads its condition on **every** update, under both modes, fills this
   gap. It is the most valuable item in this plan.
2. **`Running` has to carry an act.** A node that wants to yield without a
   running child (wait, delay, cooldown-wait, a repeat that restarts
   instantly) has nothing to report. Choices: take the act from the child, ask
   the user for an act closure `Fn(&C) -> A`, or require `A: Default`.
   Proposal: an **act closure argument** on the few nodes that yield on their
   own. `A: Default` would quietly make "busy with nothing" possible again.
3. **Invocation state is dropped on a terminal result.** Nothing survives
   between invocations. Cooldown, "run once", round-robin, and
   "resume the sequence where it left off last time" all need memory that
   outlives the invocation. Proposal: keep that memory in context, reached
   through an accessor closure. Defer any core change (see *Core changes*).
4. **Abort only runs Drop, and Drop gets no context.** A decorator can abort
   its child by dropping the child's state, which fires `CancelOnDrop`. An
   `on_abort(|ctx| ..)` hook would need a cancel traversal with context, which
   was rejected earlier. `finally` can run only on normal completion.
5. **`BtControl` already gets `&mut C` and the saved active index in `begin`.**
   That is enough for random policies (draw from an RNG in context), utility
   scoring, and hysteresis (the active child gets a score bonus). None of
   these needs a core change.
6. **One active child per control.** `BtChildren` keeps a single variant.
   Parallel execution needs a different children trait in which every child's
   state is alive at once.
7. **The policy loop has no iteration budget.** A repeat around a child that
   completes instantly hangs the update. Looping nodes need their own
   per-update bound.
8. **No dependencies.** RNG and clock come from context, not from crates.

## Candidates

Priority: **P1** = build first, fixes a real authoring gap. **P2** = common
and cheap. **P3** = useful, but costlier or needs a decision first.
Size: **S** < 100 lines with tests, **M** a few hundred, **L** needs new
traits or generated code.
Core fit: **fits** = public API only. **decision** = fits, but an open
question must be settled first. **core** = needs a core change.

### Decorators (wrap one child, implement `BtNode` directly)

| Node | Semantics | Pri | Size | Core fit |
| --- | --- | --- | --- | --- |
| `guard(cond, child)` | Checks `cond` on every update, under both modes and before first entry. False on entry: Failure, and the child never starts. False later: the child's state is dropped (cancellation runs) and the node returns Failure. Otherwise it passes the child's result through. | P1 | S | fits |
| `while_(cond, child)` | A loop. Re-checks `cond` every update. When `cond` goes false, aborts the child and returns **Success** (the loop ended normally). When the child succeeds while `cond` still holds, restarts it. Child failure: Failure. | P1 | S–M | decision (constraint 7: at most one restart per update, then yield the child's act or fail with a diagnostic) |
| `until(cond, child)` | `while_` with the condition inverted. | P1 | S | fits (alias) |
| `abort_if(cond, child)` | `guard` with the condition inverted. Reads better for interrupts. | P2 | S | fits (alias) |
| `invert`, `force_success`, `force_failure` | Standard result mapping. `Running` passes through. | P2 | S | fits |
| `map_act(f, child)` | Turns the child's `NodeResult<B>` into `NodeResult<A>`. Lets a subtree keep its own act type and be reused under a larger one. | P1 | S | fits |
| `focus(lens, child)` | Runs a subtree over `&mut D` projected from `&mut C`. Lets a subtree be reused across blackboards. | P2 | S | fits (lens closure `for<'a> Fn(&'a mut C) -> &'a mut D`) |
| `reactive(child)` | Passes `Resume` down as `Evaluate`, so this subtree reconsiders even when the root resumes. | P2 | S | fits |
| `sticky(child)` | Passes `Evaluate` down as `Resume`, so this subtree does not switch branches until it finishes. Useful under Bevy's default `Tick::Evaluate`. | P2 | S | fits |
| `repeat(n, child)`, `retry(n, child)`, `repeat_forever(child)` | Count successes or failures inside the invocation. `Repeat` in `examples/support` becomes the catalog version. | P2 | S | decision (constraint 7: per-update bound) |
| `timeout(clock, limit, child)` | Stores the start time in state. Past `limit`: abort, Failure. `clock: Fn(&C) -> T`. A variant counts updates instead of time. | P2 | S | fits |
| `chance(p, rng, child)` | Enters the child with probability `p` on fresh entry only. | P3 | S | fits |
| `cooldown(clock, period, memory, child)` | Fails while in cooldown. Stores the last completion in context through `memory`. | P3 | S | decision (constraint 3) |
| `once(memory, child)` | Runs the child successfully at most once per agent. | P3 | S | decision (constraint 3) |
| `finally(f, child)` | Runs `f(&mut C, &result)` on normal completion. Not on abort. | P3 | S | fits, but the name promises too much (constraint 4); maybe `on_complete` |

Params: `guard` and `while_` must forward scope parameters to the child. The
condition can also read them (`Fn(&C, P) -> bool`), which needs the same
`ParamValue` reborrow that `ControlNode` does. Plan for this from the first
version: a guard on a scope local, like `guard(|_, target: &Enemy| target.alive)`,
is the common case inside `scope!`.

### Action and function helpers

These are about writing leaves quickly, especially inside `scope!`.

| Helper | Semantics | Pri | Size | Core fit |
| --- | --- | --- | --- | --- |
| `do_while(cond, act)` | The most common action: `Running(act(ctx))` while `cond(ctx)` holds, then Success. Holding the condition in the action is exactly what constraint 1 asks for. | P1 | S | fits |
| `wait_until(cond, act)` | Same shape, read the other way: wait while reporting `act`. | P1 | S | fits (alias) |
| `action_fn(start, in_progress, tick)` (+ `.complete(..)`) | Builds a `BtAction` from closures, with no struct or impl. Closures receive params, so it works with `.with(local)`. | P1 | S–M | decision (closure inference across the params HRTB; spike first) |
| `leaf_with(f)`, `check_with(f)` | `leaf` and `check` whose closure also receives params (`Fn(&mut C, P)`). Today `leaf` ignores params. These are separate constructors, because a second `Leaf` impl would overlap under coherence. | P1 | S | fits |
| `produce(f)` | A synchronous output node: `Fn(&mut C, In) -> Option<T>` writes an `out` local, and `None` means Failure. Generalizes `scope::compute` to consumers placed mid-body. | P2 | S | fits (in `flatbt-scope` or behind a feature) |
| `ask(question_act, answer)` | Reports the question as its act until `answer: Fn(&C) -> Option<T>` returns `Some`, then writes it to an `out` local. This is the pattern the Bevy draft left to the catalog. | P2 | S | fits |
| `wait_updates(n, act)`, `wait_for(clock, d, act)` | Replaces `examples/support/wait_frames`. Takes an act (constraint 2). | P2 | S | fits |
| `emit(act)` | Reports `act` for one update, then Success. | P3 | S | fits |

"Sync function for scope" covers `leaf_with`, `check_with`, and `produce`.
All three run to completion within one update and read or write scope locals
through ordinary `.with(..)` bindings, so `scope!` needs no macro changes.

### Control policies (implement `BtControl`)

| Policy | Semantics | Pri | Size | Core fit |
| --- | --- | --- | --- | --- |
| `utility!(|c: &C| { score_a => a, score_b => b })` / `utility(scorer, children)` | Runs the child with the highest score. When it fails, rescores and tries the best **untried** child (a `u64` bitmask in policy state). Under `Evaluate` it rescores and may preempt. An optional `inertia` adds a bonus to `active_child_index`, which `begin` already receives, to prevent flip-flopping. Scores are `f32`; NaN counts as "skip". | P1 | M | fits. The macro reuses `__flatbt_child_indices` like `choose!`, and generates `Fn(&C, usize) -> f32`. |
| `priority(...)` (dynamic priority selector) | The same machinery with integer priorities and a stable tie-break on child order. This is the utility selector with a different score type. Build it as one generic policy (`S: PartialOrd`) with two constructors. | P1 | S (on top of utility) | fits |
| `random_select(rng, children)` | On fresh entry, picks an untried child uniformly at random. On failure, tries another untried child. Under `Evaluate`, keeps the active child, like `seq`. | P2 | S | fits (`rng: Fn(&mut C) -> u32` from context) |
| `weighted_select(rng, weights, children)` | Weighted version. `weights: Fn(&C, usize) -> f32`. | P2 | S | fits |
| `shuffle_seq(rng, children)` | A sequence in random order, without replacement, using the bitmask. | P2 | S | fits |
| `shuffle_select` | `random_select` under a clearer name. Pick one of the two names. | — | — | — |
| `if_else(cond, then, else)` | Sugar over `Choose`. | P2 | S | fits |
| `seq_any` / `try_all` | Runs all children regardless of failures. Success if any succeeded (or if all did, for a variant). | P3 | S | fits |
| `round_robin(memory, children)` | Continues from the child after the one used last time, across invocations. | P3 | S | decision (constraint 3) |
| `parallel(policy, (a, b, ..))` | All children run. Policy: all / any / race. The act comes from the first running child, or from a `merge: Fn(A, A) -> A`. | P3 | L | new children trait with generated tuple impls. Can live outside core if the tuple generator is shared. |
| `with_background(main, bg)` | UE-style simple parallel: `main` sets the result and the act. `bg` runs alongside, and its act is discarded (`bg: BtNode<C, ()>`). | P3 | M | same trait as `parallel`, two-child case only. Could ship before the general form. |

Policy bitmask limit: 64 children. A larger `FLATBT_MAX_CHILDREN` makes
`begin` report `ControlOp::error` instead of misbehaving. Use a `u128` if
that turns out too small.

## Core changes that would unlock more (all deferred)

| Change | Unlocks | Recommendation |
| --- | --- | --- |
| `BtControl::resume(state, ctx, active) -> ControlOp`, defaulting to `RunChild(active)` | A reactive sequence that re-checks preceding conditions even under `Resume`. | **Defer.** `guard` covers the need without a second meaning for `Resume`, which is the same objection that sank `continuation_failed`. Revisit if a real tree needs control-level reactivity that `guard` cannot express. |
| Per-agent persistent node memory (an associated `Memory` type kept across terminal results) | `cooldown`, `once`, and `round_robin` without an accessor into context. | **Defer.** Accessors are explicit and cost nothing. Revisit after using them. |
| Exported tuple generator for "all children alive" | `parallel`, `with_background` | Do it together with `parallel`, in core's `build.rs`. Only a codegen export, not a runtime change. |
| Context passed to abort | `on_abort` / real `finally` | **Reject.** It contradicts state-owned cancellation. |

## Plan

Each phase ends with the full check list from `CONTRIBUTING.md`, README entries
for what shipped, and a decision-log entry.

**Phase 1: guards and leaf helpers** (feature `decorators` + `fn`; P1 items, all S)

- `guard`, `abort_if`, `while_`, `until`, and `map_act`.
- `do_while` / `wait_until`, `leaf_with`, and `check_with`.
- Tests pin the key contract: `guard` aborts a suspended action under
  **`Resume`**, and its `CancelOnDrop` fires. That is the case that fails with
  `seq((check, action))` today.
- Settle constraint 7 for `while_`: at most one restart per update. If the
  restarted child completes again in the same update, log a diagnostic and
  fail.

**Phase 2: utility / priority selection** (feature `utility`; M)

- A generic scored policy, the `utility!` macro (sharing the child-index
  machinery with `choose!`), `priority`, and inertia.
- Tests: preemption under `Evaluate`, fallback on failure in score order,
  inertia preventing switches, and NaN skipped.

**Phase 3: random policies and remaining decorators** (features `random`, `decorators`; S each)

- `random_select`, `weighted_select`, and `shuffle_seq`, with an RNG from
  context. Tests use a deterministic counter RNG.
- `invert`, `force_*`, `repeat`/`retry` (moved out of `examples/support`),
  `timeout`, `reactive`, `sticky`, and `focus`.
- `action_fn` after a spike on closure inference. `ask`, `produce`, and
  `wait_updates` (replacing `examples/support/wait_frames`).

**Phase 4: needs a decision first** (P3)

- Memory-backed nodes (`cooldown`, `once`, `round_robin`) through context
  accessors.
- `with_background`, then `parallel`, with the shared tuple generator.

## Open questions

- `while_` naming. Rust reserves `while`, so the choices are `while_`,
  `loop_while`, or `repeat_while`. `do_while` (the action) and `while_` (the
  decorator) are easy to confuse. Consider `hold_while` for the action.
- Should `guard` return Failure or Success when its condition goes false
  mid-run? This plan says Failure, so a parent `select` falls through to the
  next branch. `while_` covers the Success case.
- Feature granularity: one `catalog` feature, or one per group (`decorators`,
  `utility`, `random`, `fn`)? The existing crate uses one per group.
- Should the utility scorer see scope params (`Fn(&C, P, usize)`)? That would
  need the policy to receive params, which `BtControl` does not do today.
