# Implementation decisions

## 2026-09-07 — M0: executable synchronous backend

The original document is a source of ideas, not an immutable specification. This
log records implemented decisions, deviations, and questions for later iterations.
Where the source document differs, the code, tests, and this log describe the
semantics currently implemented.

### Decisions for the first iteration

- One library crate, `flatbt`, using Rust edition 2024 with no external dependencies.
  Validated on Rust 1.98.1; the minimum supported Rust version is not yet defined.
- Tree definitions are immutable (`&self`); mutable application data is passed through
  `&mut C`. One definition can execute sequentially with different contexts.
- `BtNode::update(&self, &mut C) -> NodeResult` is currently synchronous. Defer
  `ExecutionCursor`, `EntryMode`, and persistent state until an executable M1 scenario
  can validate them. This signature is temporary; the resumable protocol remains planned.
- `BtControl<C>` is a separate policy, not a node. `ControlNode` owns the execution
  loop; the policy selects the next index. A callback runs after a child's terminal
  result. `State: Default` is created for each invocation. `Send` is not required yet
  because state does not survive an update. Revisit persistent state bounds in M1.
- Children are ordinary tuples of arity 0–32. A macro generates a direct call to the
  concrete child in each `match` arm. This iteration uses no trait objects, unsafe
  code, or frame storage.
- A sequence stops at the first failure; a selector stops at the first success.
  Empty controls have identity results: sequence success, selector failure.
- A custom policy can select a child again after its terminal result in the same
  update. The policy must ensure termination; an invalid index causes a clear panic.
- Context changes are not rolled back after failure. Side effects during future
  speculative execution need further discussion; no commit or tick guarantee exists yet.

### Evidence in code

`tests/synchronous.rs` covers execution order and short-circuiting, nested
heterogeneous composition, repeated updates, different contexts, custom policies
with fresh state, zero repetitions, invalid indices, and dispatch at all 32 positions.
`examples/synchronous.rs` executes combat/patrol/idle and includes a custom `Repeat`.

### Next iteration — M1

Implement `Running` without a mandatory tick, boxed storage for the active path,
and normal resume. The executable scenario should evaluate a condition once,
suspend in wait, and execute fire in the same update in which wait completes.

Validate these questions in code before settling the API:

1. How does a safe child-entry API separate execution traversal from typed frame storage?
2. How are definitions bound to runtime state so that state cannot reach the wrong node?
3. How does an invocation end and release its state, including terminal results and Drop?
4. How does a fresh entry receive Evaluate while a saved continuation receives Resume?

Memoryful sequences during root revalidation, reactive selectors, and preserving
an old branch while evaluating alternatives belong to M2. Synchronous M0 does not
validate those behaviors yet.

## 2026-09-07 — M1: boxed invocation state and normal resume

This iteration supersedes the synchronous-only API and panic behavior described in M0.

### Execution and storage

- `BtNode` now has `State: Default + Send + 'static` and a single `update` method
  receiving typed state, context, `ExecutionCursor`, and `EntryMode`. Default state
  keeps this prototype simple; context-dependent resources can be initialized in
  an optional state field during execution.
- `BtState::new(&tree)` binds an instance to one borrowed definition and context
  type. The definition cannot be replaced through the instance API. Separate
  instances share definitions and own independent continuation state.
- `FrameStorage` only owns a boxed typed value and supplies checked `Any` downcasts.
  Execution owns the invocation layout, child index, and mode selection. One Box
  contains each invocation's state and child slot. Only the active path survives
  between updates; synchronous invocations also allocate temporarily in this
  reference backend. No unsafe code or node trait-object dispatch is needed.
- Fresh frames enter as Evaluate; existing frames enter as Resume. Controls skip
  `policy.begin()` on Resume and follow their framework-owned active child.
- Terminal results drop the invocation and its descendants. Selecting the same
  index again creates fresh state and enters as Evaluate, even within one update.
  A completed child can be followed by another child immediately.
- Dropping or resetting a `BtState` drops its saved path. Invocation fields are
  ordered so descendants drop before parent state. There are no cancellation hooks.
- `wait_frames(n)` suspends for exactly `n` updates, then succeeds on the next.
  Custom nodes can also suspend without any tick capability.

### Recoverable errors and focused tests

- Ordinary BT Failure remains silent. `NodeResult::error` and `ControlOp::error`
  explicitly log a diagnostic and return Failure. Invalid child indices follow
  the same rule in debug and release. A selector can then attempt its fallback.
- Diagnostics use stderr for now; a failed write is ignored. Integration with an
  application's logging system is a later API decision. We do not catch arbitrary
  user panics or implement an execution budget in this iteration.
- Panic tests and the large tuple-position test were removed. Tests focus on
  observable behavior: ordered execution, fallback, suspension, saved selection,
  fresh invocation state, and ownership on completion/reset/drop. One error-path
  scenario exercises recovery through a selector rather than testing panic details.
- `examples/resume.rs` runs `check → wait_frames(3) → fire`: three Running results,
  then Success, with one check and one shot.
- Validation: 11 behavioral tests and one doctest pass in debug and release;
  both examples run successfully, and Clippy and formatting checks pass.

### Boundaries for M2

Root revalidation is not exposed yet. The current Evaluate entries are fresh;
existing invocation + Evaluate, memoryful sequence revalidation, speculative
alternatives, and preemption will be implemented together in M2. The current child
slot holds one path and will need to preserve an old path while evaluating a
candidate. Storage must remain separate from those execution decisions.

Custom composition currently uses `BtControl` and stable tuple children. The cursor's
direct child-entry helper remains internal. Custom implementations must not swap
child definitions behind a persisted slot; dynamic composition is outside M1's
identity contract and needs explicit identity rules before it is supported.


## 2026-09-07 — Review correction: Sequence Evaluate semantics

Review exposed a missing part of the policy contract: `begin` had no access to
continuation metadata, so Sequence always selected child zero on Evaluate. M1's
normal Resume bypassed `begin`, and the existing tests only exercised Evaluate on
fresh invocations. That left the low-level existing-invocation Evaluate contract
incorrect even though full root revalidation was deferred.

`BtControl::begin` now receives `active_child: Option<usize>` by value. The
framework retains ownership of that metadata. Sequence selects the saved child
when present; Selector deliberately restarts its priority scan at zero. The
policy's own State can remain `()` because continuation is framework-owned.

One regression scenario enters existing controls with Evaluate through a small
low-level node adapter. Changing the earlier condition after suspension must not
replay it in Sequence, while Selector must inspect it again. The test failed on
the previous implementation and passes with the corrected contract. All 12
behavioral tests and the doctest pass. This test covers the control's entry
semantics; it does not implement or validate full M2 candidate preservation.
