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

## 2026-09-17 — Bevy: the blackboard is an ordinary component

`flatbt-bevy` behind the `bevy` feature. A tree over `C` is ticked by a system
whose query is `(&mut Behavior<C, F>, &mut C)`, where `C` is a plain component
the game defines. `BehaviorPlugin::for_tree(builder)` builds the tree once into
a resource and adds that tick; agents carry `Behavior::for_tree(builder)`, whose
only content is the invocation state, sized for that tree. Both type parameters
come from the builder function, so neither is ever written out.

The crate does not describe what a blackboard holds, how it is filled, or what a
value written into it means. Two findings put it there, after two earlier shapes
(live borrows, then a `BehaviorContext` with `read`/`write` producing a
snapshot) were built and removed:

- A real gather is several systems at several rates -- one for what is cheap,
  another for a raycast, another for a path query only agents already in combat
  should pay for. A single `read` function cannot express that; ordinary systems
  with `.run_if(..)` can.
- What a tree writes is however that game controls its agents, which no library
  can guess.

Answering both by deletion took the crate from 436 lines of code to 208, and
measured back to back over 100 000 agents in `games/arena` the tick is about
1.9x cheaper serially and 2.4x in parallel, and the whole frame 18% and 8%,
because the blackboard is read and written in place rather than gathered into a
temporary per agent and applied back.

Given up, deliberately: a node cannot defer a world edit (there is no
`bb.commands`; a tree writes a request field and a system answers it); there is
no warning for an unregistered tree, since the tick query no longer has anything
to hang one on; `entry_mode` is `fn(&C) -> EntryMode` with no `Entity`; and the
agent query is no longer a turn gate, so a turn-based game gates from the
blackboard instead.

`Behavior::tick` re-enters once with `EntryMode::Evaluate` when a tick entered as
`Resume` fails at the root, which is the caller-side half of the 2026-09-16
decision above. Entry mode defaults to `Evaluate`: resuming is an optimisation,
and a tree that only ever resumes never leaves the branch it is in.

`Behavior::tick` and `BehaviorTree` are public, so a game that needs its own tick
system writes one; the only thing it cannot write for itself is the generic that
names the tree, because a composed tree's state type and a builder's type are
both unnameable.

See [Bevy integration](bevy-integration-draft.md).

## 2026-09-17 — Bevy: a tick a tree cannot skip for itself, a read-only input, and `ask`

Three follow-ups to the entry above, each settled by writing the alternatives
and measuring them rather than by argument.

**`Tick::Skip`.** The plugin's `entry_mode` becomes `tick_mode`, and its answer
becomes `Tick::{Skip, Resume, Evaluate}`. `Skip` does not enter the tree and
leaves a suspended invocation exactly as it was. It is not derivable from a
guard inside the tree, and all three shapes that look like they should work are
pinned failing in `crates/flatbt-bevy/tests/entry.rs`: a guard as child zero of
a `seq` is never consulted again under `Resume` (the active child is re-entered
directly) nor under `Evaluate` (`seq` continues its active child rather than
rescanning), and under a `select`, which does rescan, a candidate that fails
leaves the standing branch in place and runs it. Failing a candidate is how a
tree redirects, not how it stops.

Over 200 000 agents with nine in ten idle, serial tick: 2.19-2.24 ms with every
agent entering the tree, 1.49-1.50 ms with nine in ten failing a root guard,
0.62-0.63 ms with nine in ten skipped. In `games/arena`, `NOSKIP=1` against the
default over 100 000 agents: 1.28-1.41 ms against 0.72-0.78 for the tick, and
2.31-2.45 against 1.67-1.80 for the whole frame. It also replaces the root
guard as the turn-based gate, and unlike the guard it holds across a turn that
spans several ticks.

**`Split<In, Out>`.** An optional blackboard whose input half is reachable by
`Deref` and nothing else; writing goes through `out()`. Both fields are private,
so the rule crosses the module boundary, and a `compile_fail` doctest pins it.
It costs nothing measurable and every constructor composes over it unchanged.
The gather still needs `&mut In` and no Rust type can tell a gather system from
a node, so `sensed_mut()` is public: what the split buys is that no node reaches
the input by accident.

Rejected for the same job: two components in the tick's query, which needs a
struct holding two borrows and brings back the higher-ranked bound; and the
output as the root's `params`, where the mechanism works -- `P` does reach a
leaf through every control node -- but `update` and `BtState` fix the root's `P`
to `()`, `leaf` and `check` ignore `P` so every write becomes a hand-written
`BtNode`, and `scope!` owns `P` for its locals.

**`ask` returns, in `flatbt-nodes`, with `Request<T>`.** With the question and
the answer both fields of the context, `ask` is two closures over `C` and knows
nothing about the ECS, so it is a catalog node rather than a Bevy one.
`Request<T>` (`Idle`, `Pending`, `Answered(T)`) keeps both in one field, and the
system answering it matches on `Pending` rather than re-deriving who wants an
answer, so that condition stays in the tree that decided it.

The question cannot be a `scope!` local: a local lives in the invocation state,
whose type no system can name. And a node cannot run the query itself, which was
checked three ways and fails on the language rather than on this crate -- a tree
is built once into a resource and is `'static`, while a `Query<'w, 's, ..>`
borrows the world for one system run. Through the context is shape 1 again;
through `params`, `&'a Query<'w, 's, ..>` is two levels of lifetime and
`for<'a, 'w, 's>` over it does not resolve ("implementation of `BtNode` is not
general enough", reproduced in a five-line probe with no Bevy in it); and a
`&'a dyn Perception` that would hide those lifetimes is blocked by `T: 'static`
and `T: Sized` on `ParamValue for &T`. Relaxing those is the one escape hatch
worth revisiting if a consumer needs it, since it is a bounded change to the
parameter machinery rather than a return to shape 1. `request` runs once per
invocation and `is_in_progress` holds until `answered` returns a value; bound to
a `scope!` output slot it fills the local, so the node after it takes a value
rather than an `Option` and the answer does not outlive the decision that wanted
it. The action shape is the point: a leaf returning `Running` is re-entered on
every resume, which was the arena's 220 ns-per-agent bug.

**No command channel, and why that is measured.** `Commands` borrows the world
and would put lifetimes back into every node signature. `CommandQueue` does not,
so a game can put one in its own blackboard and drain it after the tick, in
about twenty lines; `crates/flatbt-bevy/tests/commands.rs` keeps a working copy
with the price. Over 200 000 agents an unused queue costs 70% of the tick (the
blackboard grows by 56 bytes per agent, and the drain is another pass), and a
queue every agent writes to costs twenty times the frame. It suits the rare
structural edit and never what an agent decides every tick, which is why it is a
documented pattern with a number attached rather than an API that looks free.
