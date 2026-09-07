# FlatBT

An experimental Behavior Tree runtime in Rust. Development proceeds in small,
working iterations: validate semantics with statically composed nodes and state,
then add dynamic boundaries and a compiled frontend.

The current implementation supports synchronous composition, suspension, normal resume,
root re-evaluation, and preemption. It is based on the simple M1 implementation;
see the [draft design notes](docs/design/static-state-draft.md) for state composition.

```rust
use flatbt::{BtState, EntryMode, NodeResult, check, leaf, seq, update};

let tree = seq((
    check(|ammo: &usize| *ammo > 0),
    leaf(|ammo: &mut usize| {
        *ammo -= 1;
        NodeResult::Success
    }),
));

let mut state = BtState::new(&tree);
let mut ammo = 1;
assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Success);
assert_eq!(ammo, 0);
```

`BtState::new(&root)` creates state bound by reference to the root definition.
The free `update` function takes the root, state, context, and entry mode. It checks
the root binding before executing. Completion and `reset()` discard saved state
while preserving the binding. One root can serve multiple independent instances
and must outlive them. The node trait has no state-construction method.

`EntryMode::Resume` follows the saved path. `EntryMode::Evaluate` revalidates from
the root: Sequence preserves its active child, while Selector scans from child
zero. Fresh invocations always receive Evaluate, regardless of the requested mode.
A failed candidate leaves the old branch state intact; a new Running candidate
preempts it. A terminal result releases the invocation, so the next update starts fresh.

Each node's `State` includes the state of its statically known descendants.
`ControlNode::State` combines policy state, the active child index, and a tuple of
typed optional child states. Tuple dispatch borrows the selected child's field
directly. Unvisited children remain uninitialized. Failed candidates clear their
own fields; a new Running candidate clears the previous selection's field.

The runtime has no frame stack, scratch, type erasure, or storage backend. Static
state layout is known to Rust, and the runtime adds no heap allocations. A custom
node can still own allocating resources in its state. The current product layout
reserves space for every child; a compact sum layout is a later optimization.
Frame storage and layout descriptors are deferred to dynamic node boundaries.

Examples:

```sh
cargo run --offline --example synchronous
cargo run --offline --example resume
cargo run --offline --example revalidation
```

The resume example uses an application-defined `WaitFrames` from
`examples/support/wait_frames.rs`, shared with tests. It suspends for three updates
and executes the next child on completion. The revalidation example preserves a
patrol while a higher-priority candidate fails, then preempts it when that candidate
becomes eligible.

Core provides `BtNode`, `BtControl`, and composition primitives: `seq`, `select`,
`check`, and `leaf`. Tuple children of arity 0–32 use static dispatch. A reusable
catalog of utility nodes and policies belongs in a separate crate if introduced
later. Example helpers are not core exports.

Validation:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
```

Custom nodes use `State: Default + Send + 'static`, separate from their definition.
A composing node includes nested state fields and calls a child with the chosen
field: `child.update(&mut state.child, ctx, mode)`. It owns initialization and
cleanup when nested invocations start, finish, or are replaced.
A node can suspend without a tick capability. Empty sequences succeed; empty
selectors fail. Ordinary Failure is silent; `NodeResult::error` and
`ControlOp::error` report execution errors to stderr and return Failure.

Context changes take effect immediately and survive failed branches. Post-commit
Tick is not implemented yet. User-code panics are not caught; after an unwind,
reset the state before using it again. Custom policies must ensure termination;
there is no execution budget.

The API is experimental. Dynamic composition and its storage, `BtTick`,
`BtAction`, and the `bt!` compiler remain future work.

The [decision log](docs/design/decisions.md) records earlier iterations.
The [original architecture document](docs/design/original-architecture.md) is an
unmodified Russian source snapshot for discussion, not a binding contract.
