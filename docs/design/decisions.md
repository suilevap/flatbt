# Implementation decisions

Chronological record. Later entries supersede earlier ones. See the
[README](../../README.md) for current behavior; the
[original proposal](original-architecture.md) is historical reference.

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
See [package layout](package-layout-draft.md) for dependencies, migration, and checks.

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

## 2026-09-22 — A guard is asked on every update

`guard(cond, child)` puts that condition back outside the action. It checks
`cond` on every update, `Resume` included, and fails without entering `child`
when it does not hold, so a running child ends the moment its condition stops
holding. Checking only on `Evaluate` was rejected: `Resume` would then keep a
child running past its condition, and a tree must not behave differently under
`Resume` once it runs. `check` keeps its meaning -- asked once, on entry.
