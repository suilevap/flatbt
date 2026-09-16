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

## 2026-09-16 — A lost continuation is a fresh decision

`Resume` skips `begin()`, so no child above the active one is consulted. While
the continuation holds, that is the point. When the resumed child *fails* it is
not: the policy then chooses among children it never looked at. A `select` whose
resumed branch failed would take the branch below it even when a higher-priority
one had become available meanwhile -- and if that branch went Running, it held
the continuation for good, because nothing ended to force an `Evaluate`.

`BtControl::continuation_failed` is called instead of `child_failed` when the
failed child was the resumed continuation. It defaults to `child_failed`, so
`Sequence` and `Choose` are unchanged: a sequence has no priority to restore,
and a choose fails outward and re-picks on the next update. `Selector` overrides
it to rescan from child zero.

`ControlNode` will not run the failed child a second time in the same update:
the rescan reaches it, and it has already failed, so its result is reused rather
than its effects repeated.

Found by `games/arena`, where the symptom was an agent that never reconsidered.
Tests: `selector_rescans_priority_when_its_resumed_branch_fails` and
`a_failed_continuation_is_not_rerun_by_the_rescan` in `tests/resume.rs`.
