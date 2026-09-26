# Implementation decisions

Chronological record. Later entries supersede earlier ones. See the
[README](../../README.md) for current behavior; the
[original proposal](archive/original-architecture.md) is historical reference.

## 2026-09-07 — M0: synchronous backend

- One `flatbt` crate, Rust edition 2024, no external dependencies. Validated on
  Rust 1.98.1; minimum supported version undefined.
- Immutable definitions (`&self`), mutable context (`&mut C`), sequential reuse
  across contexts. Temporary API: `BtNode::update(&self, &mut C) -> NodeResult`.
- `BtControl<C>` selects indices; ControlNode owns execution. Policy state uses
  Default per invocation. No Send bound before suspension exists.
- Tuple arities 0–32 dispatch directly to concrete children. No trait objects,
  unsafe code, or frames.
- Sequence stops on Failure; Selector stops on Success. Empty sequence succeeds;
  empty selector fails. Context writes survive failure.
- Policies may repeat a child in one update and must terminate. Invalid indices
  panic in this iteration; M1 changes this to recoverable failure.

Validation: synchronous tests covered order, short-circuiting, heterogeneous
nesting, repeat updates, different contexts, custom policies, zero repetitions,
invalid indices, and all 32 positions. The example ran combat/patrol/idle and Repeat.

Next: boxed active-path state and Running, demonstrated by
`check → wait_frames(3) → fire`. Resolve typed child entry, definition/state binding,
invocation cleanup, and fresh Evaluate versus saved Resume. Defer root revalidation,
candidate preservation, and preemption to M2.

## 2026-09-07 — M1: boxed state and resume

Supersedes M0's synchronous API and panic behavior.

- BtNode adds `State: Default + Send + 'static`, typed state,
  ExecutionCursor, and EntryMode. Optional fields support context-dependent setup.
- `BtState::new(&tree)` binds one borrowed definition and context type. Instances
  own independent state and may share definitions.
- FrameStorage owns boxed typed values with checked Any downcasts. Execution owns
  layout, child index, and mode. One Box contains invocation state and child slot;
  even synchronous invocations allocate temporarily. No unsafe or node trait objects.
- Fresh frames enter Evaluate; saved frames Resume. Resume skips policy.begin.
- Terminal results drop invocations and descendants. Repeating a completed index
  starts fresh, including within the same update.
- Reset/Drop release saved paths, descendants before parents. No cancel hooks.
- `wait_frames(n)` returns Running n times, then Success. Tick is optional.

Errors and validation:

- Ordinary Failure is silent. `NodeResult::error` and `ControlOp::error` log to stderr
  and fail, including invalid indices in debug/release. Failed diagnostic writes
  are ignored. No panic interception or execution budget.
- Tests target order, fallback, suspension, saved selection, fresh state, and
  completion/reset/Drop. Removed panic-detail and exhaustive-position tests.
- Resume example: three Running results, then Success; one check and one shot.
- 11 behavior tests, one doctest, debug/release, both examples, Clippy, and formatting passed.

M2 boundary: preserve an old path while evaluating candidates; keep storage separate
from execution decisions. Root revalidation is not exposed yet. Direct cursor child
entry stays internal. Stable tuple composition must not replace definitions behind
saved state; dynamic identity rules remain open.

## 2026-09-07 — Sequence Evaluate correction

`begin` lacked continuation metadata, so existing Sequence Evaluate restarted at
child zero. Resume bypassed begin; fresh-only Evaluate tests missed this error.

`BtControl::begin` now receives `active_child_index: Option<usize>` by value.
Sequence keeps it; Selector scans from zero. The framework owns selection, so
both policies may keep unit state.

A low-level adapter tests existing controls with Evaluate: after suspension, a
changed earlier condition is skipped by Sequence and reread by Selector. The test
failed before the fix. All 12 behavior tests and the doctest passed; full M2
candidate preservation remained untested.

Index names distinguish `active_child_index` at entry, `completed_child_index`
after a terminal result, and `child_index` during dispatch.

## 2026-09-07 — Utility nodes outside core

Moved WaitFrames/wait_frames to `examples/support/wait_frames.rs`, shared by tests
and examples through the public BtNode API. Execution semantics unchanged.
Core owns protocols and composition. Reusable Wait, priority/random selection,
throttling, and decorators belong in a future catalog crate. No catalog added yet.

## 2026-09-08 — Inline action lifecycle

Archived post-commit execution because ActiveRef, Decision, Evaluation, target
dispatch, and a second node associated type complicated composition and dynamic
boundaries. Branch `experiment/post-commit-actions`, commit `95f9e33`, base `2f1525c`;
see [experiment notes](../../experiments/README.md).

Main restores `BtNode::update -> NodeResult`. ActionNode stores `Option<A::State>`
and adapts start / is_in_progress / tick / complete. Tick runs inline, so effects
may survive parent rejection and precede old-state destruction. Immediate completion
and same-update Sequence continuation remain supported.

## 2026-09-10 — Core and optional helpers

Workspace: `flatbt-core`, `flatbt-nodes`, `flatbt-scope`, plus `flatbt` entry point.
Core owns execution/basic composition. Catalog features choose/action are independent;
scope owns locals and its DSL. No helpers enabled by default.

Supersedes single-crate packaging and deferred catalog. Execution semantics unchanged.
ChooseNode becomes a delegating wrapper so the owning crate can retain its constructor.
See [package layout](package-layout.md) for dependencies, migration, and checks.

## 2026-09-10 — All helpers by default

The `flatbt` entry point now enables choose, scope, and action by default.
Core-only users set `default-features = false`; individual helpers remain selectable.
Direct crate defaults are unchanged. Supersedes the empty entry-point defaults above.

README covers implemented APIs and common usage. CONTRIBUTING links internal design
notes, decisions, and experiments. Advanced feature configuration is collapsed in README.

## 2026-09-16 — A failed resume is the caller's to reconsider

`Resume` skips `begin()`, so no child above the active one is consulted. When
the resumed child *fails*, the policy then chooses among children it never
looked at: a `select` takes the branch below the failure even when a
higher-priority one became available meanwhile.

Considered and rejected: a `BtControl::continuation_failed` hook letting
`Selector` rescan from child zero. It fixes the case completely, but it makes
`Resume` mean two things depending on what the resumed child returned, and puts
branch-picking policy into the core loop. `Resume` stays an honest resume from
the saved position, and a driver that wants otherwise re-enters with `Evaluate`
itself -- a terminal result drops invocation state, so the next update would
have done that anyway, and re-entering now only spares an agent a tick of doing
nothing.

Pinned by `a_resumed_branch_that_fails_falls_through_below_it_not_back_above`
in `tests/resume.rs`.

## 2026-09-19 — A tree returns what the agent is doing

`NodeResult` gains the act: `enum NodeResult<A = ()> { Success, Failure, Running(A) }`,
and `BtNode<C, A = (), P = ()>` threads it. An update now says whether the
invocation ended, and if it did not, what the agent is now doing.

The shape is the point. An act cannot exist without something running, and
nothing can run without saying what it is doing -- both by type rather than by
convention, which is why the act is the payload of `Running` rather than a
second return value or an out-parameter. "Busy with nothing" and "decided
something, then failed" both stop being representable.

`BtAction::tick` returns the act and is no longer defaulted, for the same
reason: an action is what occupies the agent. It is asked on every update the
action is still in progress, which is what lets a long action follow a moving
target -- restating where it is going without ending. That is the two-level
split the design wanted: `Act::MoveTo(pos)` is the low-level order, and a
`Chase`/`TakeCover` action is the standing intent that keeps rewriting it.

The act type unifies from the nodes that decide. `check` never names it, nor
does any node that only succeeds or fails, so `fn fighter() -> impl BtNode<Fighter, Act>`
declares `Act` nowhere but there. No associated type and no registry: all nodes
of a tree already agree on `C`, and `A` rides the same inference.

Costs, accepted:

- A node that keeps the agent busy in a deciding tree must produce an act. A
  waiting node needs an `Act::Idle` or similar. This was weighed as a feature:
  an agent that is waiting is still doing something, and now it has to say so.
- `Running` carries a value, so a tree that decides nothing writes
  `NodeResult::RUNNING` (32 sites) and names its state as `BtState<_, _>` to pin
  the default act type, since nothing else in such a tree mentions it.
- `Option<A>` is copied up the stack through each control node rather than
  written through a `&mut`. Chosen deliberately: the act is conceptually part of
  the result, and an act type is a small enum.

This lands before the Bevy integration and independently of it. What it unlocks
there: the tick query becomes `(&mut Behavior, &Bb, &mut Act)` -- a read-only
blackboard and the decision as its own component -- so the `ActionComponent`
bridge and the `Split` wrapper both stop being needed.

## 2026-09-19 — Bevy, rebuilt on the act

The integration is rewritten over `NodeResult<A>`. The tick query becomes
`(Entity, &mut Behavior<C, A, F>, &mut C, Option<&mut A>)`: a tree reads its
blackboard, returns what the agent is doing, and the tick puts that on the
entity as a component.

An agent doing something carries its act; an agent whose tree ended carries
none. `Query<(&Act, &mut Transform)>` is therefore exactly the agents with a
standing order, and `Without<Act>` exactly the idle ones -- no flags, nothing to
clear, and no system reading the blackboard.

Two pieces built for the previous shape are deleted rather than ported:

- **`ActionComponent`**, which carried one decision at a time from a blackboard
  field to a component. It cost four places per action (field, component,
  hand-written `BtAction`, registration) and, because each decision was its own
  marker, an archetype move whenever an agent changed its mind -- 0.5-0.8 ms of
  frame per registered decision over 100 000 agents in `games/arena`, taking the
  frame from 1.7 ms to 4.7. One act component whose *value* changes has neither
  problem: the tick writes it in place with `set_if_neq`, and only appearing or
  disappearing costs a command.
- **`Split<In, Out>`**, the read-only-input wrapper. With the decision leaving
  through the act there is nothing in the blackboard for a tree to write, so the
  discipline it enforced is now the shape of the API.

`Tick::Skip` gains a second meaning worth stating: it leaves the standing act
alone as well as the invocation, so the systems carrying that act out keep
seeing it. That is what makes it the right gate for a turn-based game and for an
agent waiting on work the world is doing.

One authoring consequence, learned from a test that failed for the right reason:
whatever keeps an agent busy is what decides when to stop. A `check` above a
running node is not consulted again under any entry mode, so the condition
belongs in `is_in_progress` on the action, not in a guard above it.

## 2026-09-22 — `tick_mode` receives the agent and the clock

`TickFn<C>` was `fn(&C) -> Tick`, so `evaluate_every` and `act_every`, which need
an `Entity` and a clock, could only be called after a gather had copied both into
every blackboard. It is now `fn(&C, TickAt) -> Tick`, where `TickAt` carries the
agent's `Entity` and the schedule's `Time` elapsed and delta, read once per tick
system. `bevy_time` moves from a dev-dependency to a dependency for it; without
a `Time` resource both durations are zero.

## 2026-09-22 — A guard is asked on every update

`guard(cond, child)` moves the stopping condition from the 2026-09-19 entry
back outside the action. It checks `cond` on every update, `Resume` included,
and fails without entering `child` when it does not hold, so a running child
ends the moment its condition stops holding. Checking only on `Evaluate` was
rejected: `Resume` would then keep a child running past its condition, and a
tree must not behave differently under `Resume` once it runs. `check` keeps its
meaning -- asked once, on entry.

## 2026-09-22 — One crate, plus the Bevy integration

`flatbt-core`, `flatbt-nodes` and `flatbt-scope` merge into `flatbt` as modules
(`runtime`, `composition`, `params`, `nodes`, `scope`), and the `choose`,
`scope`, `action` and `bevy` features are removed. Supersedes the two
2026-09-10 entries.

The split treated crates like projects in a solution, but a crate is a unit of
dependencies and release. None of the three had a dependency or a release cadence
of its own; the split cost five manifests, hidden `__private` re-exports for the
macros, a wrapper for `ChooseNode`, and a feature matrix script. `flatbt-bevy`
stays separate: it depends on Bevy and exposes its types, so a Bevy upgrade is a
breaking release of it alone. It can no longer be re-exported as `flatbt::bevy`
(that would be a dependency cycle); its prelude re-exports `flatbt::prelude`
instead.

Two features stay: `extras`, gating `nodes` and `scope` as the optional part
of the library, and `std`, whose absence makes the crate `no_std`. Both default
on, and CI checks the two ends. Without `std`, diagnostics are discarded:
holding a settable handler would take a lock or unsafe code.

## 2026-09-23 — Node catalog, first phase

From the [node catalog proposal](node-catalog-draft.md): `repeat_while`,
`map_act`, `action_while`, `leaf_with` and `check_with` join `flatbt::nodes`
under `extras`. `guard` had already landed in composition.

`repeat_while` is a goal where `guard` is a requirement: a false condition
succeeds it, so it reads as the prerequisite for the next node in a sequence.
It fails only when the condition still holds and the child can no longer keep
the agent busy -- the child failed, or completed without returning `Running`.
The second rule is what bounds the loop: a restart runs in the same update, and
a restarted child that completes at once would otherwise spin with no act to
report, so it fails with a diagnostic. No iteration budget is needed.

`map_act` carries the child's act type as a phantom parameter; a type that only
appears in bounds would leave the impl unconstrained. `leaf_with` and
`check_with` are separate constructors because a second `Leaf` impl over
`Fn(&mut C, P)` would overlap the first under coherence.

## 2026-09-23 — Conditions that read scope parameters

`guard`, `repeat_while` and `action_while` take a condition (and an act) that
is either `Fn(&C)` or `Fn(&C, P)`, where `P` is a reborrow of the parameters
the node forwards to its child. So a target picked into a `scope!` local is the
one the condition asks about on every update, under the same name.

One constructor takes both through `ReadFn<C, P, R, M>`, implemented for each
closure shape with its own marker type: the two impls are of different traits
(`ReadFn<.., ReadsContext>` and `ReadFn<.., ReadsParams>`), so they do not
overlap the way two `BtNode` impls on one node would, and the marker is
inferred from the closure's arity. Nodes carry it as a phantom parameter.

Considered first: separate `guard_with`, `repeat_while_with` and
`action_while_with` constructors. Rejected for doubling the API, and because
`action_while` accepted `.with(..)` and silently ignored it.

Costs, accepted: `guard` now needs `P: ParamValue`, as `seq` and `select`
already do; an unmatched closure reports `ReadFn` rather than a plain `Fn`
mismatch, softened by a `diagnostic::on_unimplemented` note. `leaf_with` and
`check_with` stay separate for now: `leaf` and `check` would take the same
treatment, but that is a core change on its own.

## 2026-09-23 — `with(...)` may come first

In `scope!`, `with(target) guard(.., seq((..)));` binds the node after it the
same way `guard(.., seq((..))).with(target);` does. A long node pushed the
binding to its last line, where a reader meets it after the closures that use
it. Only the macro can offer this: `target` names a local, not a value, so a
function or method taking it first cannot exist outside `scope!`.

Both forms stay. The prefix suits long nodes, the suffix short ones such as
`action(Walk).with(pos)`, and keeping the suffix breaks no tree. `with` at the
start of a node is reserved inside `scope!`; `with(x);` with no node is a
compile error.

## 2026-09-23 — Selection by score

`utility!` and `Utility<F, S>` run the child with the highest score and fall
back to the best untried one when it fails, tracking tried children in a `u64`
per invocation (so at most 64 children). Evaluate rescores and may preempt;
Resume continues without scoring, like `select`.

- **One policy, generic score.** The proposal's separate `priority` selector
  was the same policy with integer scores, so it is not a second name.
- **Ties keep the running child.** Otherwise a challenger that only matches the
  running child plus its inertia would take over, contradicting what inertia
  promises; a test caught exactly that.
- **NaN skips a child**, detected as a score not comparable with itself, which
  keeps the policy generic over `PartialOrd`.
- **Scorer reads the blackboard only**, as decided in the proposal: `BtControl`
  does not receive parameters.
- The macro's last arm becomes the scorer's `_` arm, so no unreachable branch
  is generated and nothing can panic.

## 2026-09-24 — Order is a layer over children; the remaining decorators

Phase 3 of the node catalog, reshaped in review. Supersedes the `Utility`
policy of the 2026-09-23 entry on selection by score.

**Order separates from control.** `utility`, `random_select`,
`weighted_select` and `shuffle_seq` were each a control that chose a child and
then behaved like `select` or `seq`. They are now one wrapper and three orders:
`order_by(order, children)` implements `BtChildren`, so the ordinary `select`
and `seq` visit the children in the order a `BtOrder` computes.

| Before | Now |
| --- | --- |
| `utility(score, ..)` | `select(order_by(by_score(score), ..))` |
| `random_select(rng, ..)` | `select(order_by(shuffled(rng), ..))` |
| `weighted_select(rng, w, ..)` | `select(order_by(weighted(rng, w), ..))` |
| `shuffle_seq(rng, ..)` | `seq(order_by(shuffled(rng), ..))` |

Combinations that had no node come free: `seq(order_by(by_score(..)))` runs
everything best first. The four old names stay as shorthand functions, each
exactly the composition in its row, so the common cases stay short while
`order_by` is the one implementation.

- **Controls stay unchanged.** Wrapping the children rather than the policy
  means no `select_by`/`seq_by`: the control is the one already known, and so
  is its reaction to `Evaluate`.
- **No stored permutation.** `select` and `seq` only ask for position 0, the
  running position, or the next one, so the wrapper keeps a `u64` of children
  used at earlier positions and the current position's child. Other access,
  such as `choose!` over it, reports a diagnostic and fails.
- **When the order is recomputed** follows from the control: whenever it goes
  back to position 0 under `Evaluate`. `select` does on every `Evaluate`, so a
  score order preempts; `seq` does only while on its first child.
- **Random orders use the caller's generator only.** One draw from
  `rng: Fn(&mut C) -> u32` per position. A first version seeded the invocation
  once and hashed (seed, position) so `Evaluate` could replay the order without
  storing it; review preferred not to ship a generator of our own. Instead a
  random order keeps the running child first when `Evaluate` restarts a pass,
  so a random choice holds while it runs and nothing is replayed. The rest of
  the pass is drawn afresh, so children that failed before the running one
  may be tried again.
- **`per_child!` replaces `utility!`.** `utility!` added no feature: it only
  put each score next to its child instead of in one `match index`. `weighted`
  has the same need and `shuffled` none, so the macro became order-agnostic:
  `per_child!(|bb: &C| { value => node, .. })` returns `(values, children)` for
  any order that takes `Fn(&C, usize) -> T`. Order modifiers such as
  `.inertia(x)` stay ordinary method calls. The utility case costs a `let`.
  An `order_by!(by_score, ..)` that injects the closure was rejected: it cannot
  take a method chain.
- **Too many children is a build error.** The count is static, so
  `order_by` asserts it in a `const` block rather than checking every update.
- **Randomness from the context** keeps the crate dependency-free and `no_std`,
  lets one seeded generator serve a game, and keeps tests deterministic.

The remaining decorators:

- **`repeat` and `retry` restart in the same update**, bounded by their count,
  so they need no act of their own and cannot spin. `if_else` is `choose!` with
  two arms and a condition.
- **`reevaluate_when` needs a condition.** The proposal's unconditional
  `reactive` was rejected in review; this converts `Resume` only on the
  updates the caller names.
- **`focus` takes its lens bound at construction**, `Fn(&mut C) -> &mut D`, so a
  closure returning a borrow infers its lifetimes.
- **`action_fn` and `produce` dropped** after a spike: see the proposal.
- **One file per node family** under `src/nodes/`, orders under `order/`.
  Rust has no rule either way; the old `decorate.rs` had become a grab bag.
- **Inlining: hints in the catalog, forcing in core.** Everything in
  `flatbt::nodes` is `#[inline]`, a hint LLVM weighs against its own cost
  model, rather than `#[inline(always)]`. Core keeps forcing its controls,
  leaves and `guard`, as chosen in the core inlining change (PR #18), since
  those are on every tree's path. A catalog node LLVM declines to inline
  becomes one call in an otherwise flat tree; if a benchmark shows that
  mattering, force the node that shows up rather than all of them.
  Diagnostics go through a `#[cold]`, `#[inline(never)]` function, so their
  formatting stays off the path every update takes.
