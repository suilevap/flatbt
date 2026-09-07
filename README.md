# FlatBT

An experimental Behavior Tree runtime in Rust. Development proceeds in small,
working iterations: validate semantics in the generic runtime first, then add
storage and a compiled frontend.

Currently implemented: **M0 — synchronous composition**.

- `BtNode` with `Success` / `Failure` results;
- `seq`, `select`, `check`, and `leaf`;
- `ControlNode<P, Children>` and a public `BtControl` trait for custom policies;
- heterogeneous tuple children of arity 0–32 with static dispatch.

```rust
use flatbt::{BtNode, NodeResult, check, leaf, select, seq};

let tree = select((
    seq((
        check(|ammo: &usize| *ammo > 0),
        leaf(|ammo: &mut usize| {
            *ammo -= 1;
            NodeResult::Success
        }),
    )),
    leaf(|_: &mut usize| NodeResult::Failure),
));

let mut ammo = 1;
assert_eq!(tree.update(&mut ammo), NodeResult::Success);
assert_eq!(tree.update(&mut ammo), NodeResult::Failure);
```

Run the combat/patrol/idle example, which includes a custom `Repeat` policy:

```sh
cargo run --offline --example synchronous
```

Validation:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
```

Every `update` currently starts a fresh invocation and runs to a terminal result.
An empty sequence returns `Success`; an empty selector returns `Failure`. Context
changes take effect immediately and survive a failed branch. The `fire` example
demonstrates synchronous execution only; post-commit guarantees for effects will
arrive with `BtTick`. Custom policies must ensure their execution loops terminate;
there is no execution budget yet.

The API is experimental and will change. `Running`, resume/revalidation, frame
storage, `BtTick`, dynamic behaviors, and `bt!` are not implemented yet. The next
iteration is M1: `check → wait_frames(3) → fire` across multiple updates.

The [decision log](docs/design/decisions.md) describes the current implementation.
The [original architecture document](docs/design/original-architecture.md) is an
unmodified Russian source snapshot retained for discussion, not a binding contract.
