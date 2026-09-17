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

## 2026-09-16 — The Bevy blackboard is a snapshot

An earlier draft held the update's borrows: five lifetimes, a higher-ranked
bound on every tree, `BehaviorNode` carrying an associated-type equality across
that family, and the constructor changes above so closures could be inferred
against it. It also fixed a ceiling: a node holding live borrows cannot outlive
the system run.

It holds a plain struct instead. `BehaviorContext` has `Snapshot`, `read` and
`write`; the tick is gather, run, write back. `Blackboard<C>` has no lifetimes,
`BehaviorNode` is an ordinary bound, `BtAction<Blackboard<C>>` is writable
directly, and node parameters work, so `scope!` composes. A tree is a value that
takes a value, so it can be exercised with no `World` at all.

Cost, measured over 100k agents: 4.1 ms serial and 1.9 ms parallel against
4.2 / 1.9 borrowed — once the per-agent `CommandQueue` was made lazy. An owned
empty one per agent per tick was a fifth of the whole tick: dropping a
`CommandQueue` walks its buffer whether or not anything is in it.

`read` takes the entity, so anything derived is decided once per agent per tick
rather than in each node that wants it.

## 2026-09-16 — `write` runs only for a tree that wrote

`write` ran every tick, so a context that assigned unconditionally marked its
whole population changed and dragged the rest of the engine along.
`set_if_neq` answers that per field, but nothing enforced it and the call
happened regardless.

`Blackboard` sets a flag in `DerefMut`. Reading a snapshot field goes through
`Deref`, writing one through `DerefMut`, so the tick knows whether the tree
touched anything and skips `write` when it did not. Same bargain as Bevy's own
change detection: taking `&mut` counts whether or not the value changed.

The snapshot field is private for it, with `snapshot()`, `into_snapshot()` and
`written()` in its place. `into_snapshot` takes `self`, and returning the
snapshot by value from the tick cost 1.5 ms per 100k agents, so the tick reads
it in place.

## 2026-09-17 — Reconsidering is the default; resuming is the optimisation

`BehaviorContext::entry_mode` defaulted to `EntryMode::Resume` on the argument
that a decision already taken should stand. That is the wrong polarity for a
default: a tree that only ever resumes never leaves the branch it is in, so
`select` never rescans and `choose!` never re-picks, and every reactive shape
silently stops being reactive. Resuming is correct exactly when the standing
decision is known to still hold, which is knowledge the tree's author has and
the library does not.

The cost that would have justified it is not there. Measured back to back over
100 000 agents on three trees, staggered revalidation and reconsidering every
tick come out the same: 2.54-2.72 ms against 2.57-2.77 ms. A tree whose
invocations end each tick has nothing to resume into; the saving lives in
long-running branches, which is also where resuming is most likely to be wrong.

The three tests about what `Resume` does now ask for it explicitly, which is
what they were always about.

## 2026-09-17 — Three smaller answers from the same review

**A node can write a Bevy message.** `Blackboard::write_message` is the channel
for "this happened" -- a shot fired, a target lost -- where a marker component
is the wrong shape: a marker has to be cleared by someone, and inserting and
removing one moves the entity between archetypes twice a tick, which at a large
population costs more than everything the tree did. The `guards` example used a
marker and now does not.

**`evaluate_every` moved to its own module.** It is a policy, not machinery:
`entry_mode` returns a mode and this returns a mode, so a game that wants a
different one writes it and never mentions this. Keeping it in `context.rs`
suggested otherwise.

**The unregistered-agent warning is debug only.** It is a development
convenience, not a guarantee -- a component means nothing without a system, here
as anywhere in Bevy -- so the `on_add` hook, and the resource that bounds its
noise, are behind `debug_assertions` and cost a release build nothing.
