# FlatBT

An experimental Behavior Tree runtime in Rust. Development proceeds in small,
working iterations: validate semantics in the generic runtime first, then add
production storage and a compiled frontend.

Currently implemented: **M1 — suspension and normal resume**.

- `BtNode` with `Success`, `Failure`, and `Running` results;
- `seq`, `select`, `check`, `leaf`, and `wait_frames`;
- custom `BtControl` policies and statically dispatched tuple children (arity 0–32);
- tree-bound `BtState` instances with boxed storage for the active invocation path;
- fresh entry as `Evaluate`, followed by `Resume` while suspended.

```rust
use flatbt::{BtState, NodeResult, check, leaf, seq, wait_frames};

let tree = seq((
    check(|ammo: &usize| *ammo > 0),
    wait_frames(3),
    leaf(|ammo: &mut usize| {
        *ammo -= 1;
        NodeResult::Success
    }),
));

let mut state = BtState::new(&tree);
let mut ammo = 1;
for _ in 0..3 {
    assert_eq!(state.update(&mut ammo), NodeResult::Running);
}
assert_eq!(state.update(&mut ammo), NodeResult::Success);
assert_eq!(ammo, 0);
```

The condition executes once. The sequence resumes its waiting child and executes
fire in the same update in which wait completes. A terminal result releases the
active path; the next update starts a fresh invocation. `state.reset()` discards a
suspended invocation through normal Rust Drop. Multiple instances can share a tree.

Examples:

```sh
cargo run --offline --example synchronous
cargo run --offline --example resume
```

Validation:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
```

A selector resumes its selected child without rescanning higher priorities. Root
revalidation is planned for M2. An empty sequence succeeds; an empty selector fails.
`wait_frames(n)` returns Running for `n` updates and succeeds on the next; it measures
updates, not wall-clock time. Running does not require a tick capability.

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
