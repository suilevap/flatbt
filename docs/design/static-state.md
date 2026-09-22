# Static state composition

Implemented draft, based on `3a09d11` (simple M1). The earlier pool implementation
is archived on `backup/before-stack-draft`.

## Ownership

A statically composed node includes descendant state in its own State. Parents
borrow fields and call children directly:

```rust
self.child.update(&mut state.child, ctx, params, mode)
```

Composers own initialization, fresh-entry Evaluate, and cleanup on completion or
replacement. No cursor, storage generic, or Reuse/Fresh selector.

## Control state

```text
ControlNode<Policy, (A, B, C)>::State
  = ControlState<Policy::State, child_state::State3<A::State, B::State, C::State>>

State3<SA, SB, SC> = Empty | Child0(SA) | Child1(SB) | Child2(SC)
```

The enum variant encodes the active index. Empty initializes no child. These are
state enums, not general-purpose node combinators.

| Dispatch | State handling |
| --- | --- |
| Existing child | Borrow saved payload in place; pass requested mode. |
| Different child | Initialize a local candidate; enter with Evaluate. Keep old payload alive. |
| Terminal candidate | Drop candidate; preserve saved variant. |
| Running candidate | Move into enum; drop old payload. |
| Terminal active child | Clear enum to Empty. |

Resume follows the active variant. Evaluate asks the policy: Sequence keeps its
active child; Selector scans from zero.

Persistent size is approximately the largest alternative plus tag/alignment.
No runtime heap allocation; user state may allocate. Candidate evaluation needs
temporary call-stack space, which accumulates with nesting. Selection may move
state. This layout guarantees neither lower update time nor immovable state.

Controls retain at most one Running child. A future Parallel control needs a
layout supporting several active children.

## Destruction and effects

ControlState stores children before policy state, so descendants drop first.
A terminal parent drops all remaining child state. Custom composing nodes own
their field order and cleanup. Cancel-on-drop resources follow ordinary lifetimes;
no cancel traversal. Context mutations are never rolled back. User panics propagate;
reset before reusing state after an unwind.

## Generation

[Core build script](../../crates/flatbt-core/build.rs) generates names and inputs
for tuple enums, dispatch, and child indices. Implementations remain in
[children.rs](../../crates/flatbt-core/src/composition/children.rs); generated input
is written to Cargo's OUT_DIR. No handwritten name table or identifier-concatenation
dependency.

`FLATBT_MAX_CHILDREN` defaults to 32. Set it under `[env]` in the consuming
workspace's `.cargo/config.toml`. Cargo regenerates when the value changes.

## Size check

Observed layout for a sequence of eight children with `[u64; 32]` state:

| Representation | Control state | BtState |
| --- | ---: | ---: |
| Previous tuple of optional states | 2128 bytes | 2136 bytes |
| Active-child enum | 264 bytes | 272 bytes |

Historical measurements, not ABI or timing guarantees. The regression compares
one versus eight alternatives with room for tag/alignment differences.

## Root API

```rust
let mut state = BtState::new(&root);
update(&root, &mut state, &mut context, EntryMode::Resume);
```

BtState holds the root reference, `Option<N::State>`, and a context type marker.
Initialize on entry; check root identity before update. A different root fails
without changing state or context. Reset clears state and keeps the binding.

## Deferred work

Frame storage, runtime layouts, capacities, and backends remain deferred to dynamic
boundaries whose state types are absent from their static parent. Preserve direct
typed access inside static subtrees. The old stack API was removed.

The future `bt!` compiler must generate state composition with definition composition.
[Actions](action.md) and [local parameters](local-state.md) use the same
static ownership model.

## Validation

Tests cover resume, revalidation, failed candidates, preemption, root binding,
cleanup, state size, and direct custom composition. A custom composer rejects a
Running candidate, then resumes the saved branch without losing progress.
