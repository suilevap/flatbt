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

## 2026-09-16 — A failed resume re-enters from the root, in the driver

`Resume` skips `begin()`, so no child above the active one is consulted. When
the resumed child *fails*, the policy then chooses among children it never
looked at: a `select` takes the branch below the failure even when a
higher-priority one became available meanwhile.

Considered and rejected: a `BtControl::continuation_failed` hook letting
`Selector` rescan from child zero. It fixes the case completely, including the
one below, but it makes `Resume` mean two things depending on what the resumed
child returned, and puts branch-picking policy into the core loop. `Resume`
stays an honest resume from the saved position.

Instead the *driver* decides. `flatbt_bevy::Behavior::tick` re-enters the tree
with `Evaluate` when a resumed update returns `Failure`. The next update would
have entered as `Evaluate` anyway -- a terminal result drops invocation state --
so this only spares the agent a tick of doing nothing, and it costs nothing when
trees do not fail.

What that does not cover, measured rather than assumed: the retry keys on the
*tree's* result, so a fallback below the failed branch that succeeds or goes
`Running` hides the failure from it. A `Running` fallback then holds priority
down for as long as it runs, because nothing ends to force an `Evaluate`. A tree
shaped that way has to say so through `entry_mode`. Three tests in
`crates/flatbt-bevy/tests/behavior.rs` pin all three outcomes, and
`a_resumed_branch_that_fails_falls_through_below_it_not_back_above` in
`tests/resume.rs` pins the core semantics being preserved.

## 2026-09-16 — The Bevy blackboard is a snapshot

`Blackboard<C>` held the update's borrows: five lifetimes, a higher-ranked bound
on every tree, `BehaviorNode` carrying an associated-type equality across that
family, and bounds removed from `check`, `leaf` and `compute` in core so
closures could be inferred against it. It also fixed a ceiling: a node holding
live borrows cannot outlive the system run.

It now holds a plain struct. `BehaviorContext` gained `Snapshot`, `read` and
`write`; the tick is gather, run, write back. `Blackboard<C>` has no lifetimes,
`BehaviorNode` is an ordinary bound, `BtAction<Blackboard<C>>` is writable
directly (the `AgentAction` wrapper added for the old signature is deleted), and
node parameters work again, so `scope!` composes. A tree is a value that takes a
value: `games/arena/tests/trees.rs` runs the game's real trees with no `World`.

Cost, measured in `games/arena` at 100k agents: 4.1 ms serial and 1.9 ms
parallel against 4.2 / 1.9 borrowed — once the per-agent `CommandQueue` was made
lazy. An owned empty one per agent per tick was a fifth of the whole tick:
dropping a `CommandQueue` walks its buffer whether or not anything is in it.

`read` takes the entity, so anything derived is decided once per agent per tick
rather than in each node that wants it.

## 2026-09-16 — `ask` fills scope locals

`ask` had one shape: a node that waits for an answer and leaves it on the
blackboard, where the nodes after it read it back as an `Option` and handle the
`None` that cannot happen. The scope DSL already had the other half -- `let
name: T;` reserves a slot and `.with(out name)` binds it -- and `Compute` is
just a node writing one, so nothing stopped an action from writing one too.

A second `BtAction` impl for `Ask`, over `&mut Option<T>` instead of `()`,
fills the slot in `complete`. The predicate returns `Option<T>` in that shape
and `bool` in the bare one, so which impl applies follows from the closure and
neither constructor nor type parameter is needed for it.

This is the ECS and the scope meeting: an ordinary system answers by writing an
ordinary component, `BehaviorContext::read` brings it into the snapshot, and
`ask` moves it into an invocation-local. Consumers take a value rather than an
`Option`, and the local dies with the invocation, so re-entering the branch asks
again rather than acting on a stale answer.

Tests: `ask_fills_a_scope_local_and_the_nodes_after_it_read_a_value` in
`crates/flatbt-bevy/tests/catalog.rs`; used for real by `take_cover` in
`games/arena/src/ai.rs`.
