# FlatBT

An experimental Behavior Tree runtime in Rust. Development proceeds in small,
working iterations: validate semantics in the generic runtime first, then add
production storage and a compiled frontend.

Currently implemented: **M1 — suspension and normal resume**.

- `BtNode` with `Success`, `Failure`, and `Running` results;
- `seq`, `select`, `check`, and `leaf`;
- custom `BtControl` policies and statically dispatched tuple children (arity 0–32);
- tree-bound `BtState` instances with boxed storage for the active invocation path;
- fresh entry as `Evaluate`, followed by `Resume` while suspended.

```rust
use flatbt::{BtState, NodeResult, check, leaf, seq};

let tree = seq((
    check(|ammo: &usize| *ammo > 0),
    leaf(|ammo: &mut usize| {
        *ammo -= 1;
        NodeResult::Success
    }),
));

let mut state = BtState::new(&tree);
let mut ammo = 1;
assert_eq!(state.update(&mut ammo), NodeResult::Success);
assert_eq!(ammo, 0);
```

A terminal result releases the active path; the next update starts a fresh
invocation. `state.reset()` discards a suspended invocation through normal Rust
Drop. Multiple instances can share a tree.

Examples:

```sh
cargo run --offline --example synchronous
cargo run --offline --example resume
```

The resume example uses an application-defined `WaitFrames` node from
`examples/support/wait_frames.rs`, shared with the tests. Its condition executes
once; the sequence resumes wait across updates and executes fire in the same
update in which wait completes.

The core provides execution protocols and composition primitives. A reusable
catalog of ready-made nodes and policies (Wait, PrioritySelect, RandomSelect,
Throttling, WhileDecorator, and similar utilities) belongs in a separate crate
if we introduce one later. Example and test helpers are not core exports.

Validation:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
```

Normal Resume follows the selected child without calling the control policy's
`begin`. On Evaluate, `begin` receives the active child: Sequence preserves its
progress, while Selector restarts at child zero. Full root revalidation through
`BtState` is planned for M2. An empty sequence succeeds; an empty selector fails.
The example helper `wait_frames(n)` returns Running for `n` updates and succeeds
on the next; it measures updates, not wall-clock time. Running does not require
a tick capability.

For custom nodes, implement `BtNode` with `State: Default + Send + 'static`. State is
separate from the immutable definition and persists while Running. Custom composition
currently goes through `BtControl`; dynamic child entry is not exposed yet.

Ordinary `Failure` is a normal behavior outcome and does not produce a log. Use
`NodeResult::error(message)` or `ControlOp::error(message)` for recoverable execution
errors: both write a diagnostic to stderr and return Failure. Invalid child indices
also fail with a diagnostic in both debug and release builds. User-code panics are
not caught. Custom policies must ensure their loops terminate; there is no execution
budget yet.

Context changes take effect immediately and survive failed branches. The examples
show decision-phase effects only; post-commit guarantees will arrive with `BtTick`.

The API is experimental and will change. Root revalidation/speculation, `BtTick`,
`BtAction`, dynamic behaviors, production storage, and `bt!` are not implemented yet.
The next iteration is M2: priority changes, state reuse, and preemption.

The [decision log](docs/design/decisions.md) records the implementation's evolution.
The [original architecture document](docs/design/original-architecture.md) is an
unmodified Russian source snapshot retained for discussion, not a binding contract.
