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
