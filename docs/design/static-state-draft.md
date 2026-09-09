# Static state composition draft

This iteration is based on `3a09d11`, the last simple M1 implementation.
The earlier pool-based iteration remains on `backup/before-stack-draft`.
These notes describe the current draft for review, not a frozen architecture.

## Static definitions imply static state

A statically composed node knows the types of its children, including their
associated State types. Its own State must include those states. Routing every
static node through a heterogeneous frame stack lost that property and forced
execution to choose state that already had a known place in the parent.

`BtNode::update` now receives only its concrete state, context, parameters, and EntryMode.
A composing node borrows a field and calls the child directly:

```rust
self.child.update(&mut state.child, ctx, params, mode)
```

No cursor, storage type parameter, or Reuse/Fresh selector participates. A custom
node owns the lifecycle of its nested state: it initializes fresh invocations,
passes Evaluate on fresh entry, and drops completed or preempted invocations.

## Enum layout for control nodes

The tuple implementation generates child dispatch and a state enum together:

```text
ControlNode<Policy, (A, B, C)>::State
  = ControlState<Policy::State, child_state::State3<A::State, B::State, C::State>>

State3<SA, SB, SC> = Empty | Child0(SA) | Child1(SB) | Child2(SC)
```

ControlState contains the child state enum and policy state. There is no separate
active child index: BtChildren reads it from the variant. The generated State1
through State32 types describe state alternatives, not node definitions; they do
not implement a general-purpose Either node combinator.

`build.rs` derives the enum, type parameter, and variant names from the child
index, using FLATBT_MAX_CHILDREN from the build environment (default 32). Consumers
can set it in their workspace's .cargo/config.toml under [env], without editing
FlatBT. Cargo tracks changes through rerun-if-env-changed. It writes only the macro invocation to
Cargo's OUT_DIR; the enum and dispatch implementation remain in src/children.rs.
This avoids a handwritten name table without identifier-concatenation dependencies
or unstable macro features.

Tuple dispatch matches the requested child index and corresponding enum variant.
An existing payload is passed directly by mutable reference to that child. When
another child is requested, dispatch initializes its concrete state in a local
variable and enters it as Evaluate. The old payload stays alive during this call.

Terminal candidates are dropped without modifying the saved variant. A Running
candidate replaces the variant, dropping the old branch. A terminal result from
the existing child clears the enum to Empty. Selection and lifetime therefore
stay within static child composition; execution has no reuse/fresh selector.

The complete layout is known to Rust. Persistent child storage is approximately
the largest alternative plus a discriminant and alignment, not the total size of
all children. The default Empty variant initializes no child state. The runtime
adds no heap allocation. User-defined state can allocate its own resources.

## Selection, lifetime, and temporary space

Resume follows the control's active variant. Evaluate asks the policy to choose
again: Sequence preserves the active child; Selector begins at zero. An existing
payload receives the requested mode; a new candidate gets Evaluate. Presence of
state does not force Resume.

Revalidation can require both the old state and a candidate simultaneously. The
candidate occupies ordinary call-stack space for the duration of its evaluation;
there is no second persistent slot or heterogeneous frame stack. Nested candidate
evaluation can accumulate temporary state along the call chain. Large states or
deep trees may therefore still need substantial call-stack space.

Selecting a candidate moves its state into the enum. Ordinary resume borrows the
saved payload in place. This representation optimizes persistent memory; it does
not promise faster updates, immovable state, or reduced peak call-stack usage.

When the parent terminates, its state is dropped, including the remaining active
child. ControlState declares children before policy state so normal destruction
releases descendants first. Custom composing states own their field ordering and
cleanup. Cancel-on-drop resources in state follow these same lifetimes; no
separate cancellation traversal is needed. Context mutations are never rolled back.

This layout fits the current control protocol, which returns immediately when a
child is Running and retains at most one active child between calls. A future
Parallel node retaining several children must use a different composed state.

## Size check

On the current development target, a sequence of eight children whose State is
`[u64; 32]` has these sizes:

| Representation | Control State | Whole BtState |
| --- | ---: | ---: |
| Previous tuple of optional states | 2128 bytes | 2136 bytes |
| Enum of active child states | 264 bytes | 272 bytes |

These are observed Rust layouts, not ABI guarantees or execution-time benchmarks.
The regression test compares one versus eight alternatives with room for tag and
alignment differences, rather than hard-coding byte counts.

## Root API

```rust
let mut state = BtState::new(&root);
update(&root, &mut state, &mut context, EntryMode::Resume);
```

BtState holds the root reference and `Option<N::State>`, plus a context type
marker. The root is initialized only on entry. The state binding is checked by
external update; a different root is rejected without disturbing the invocation.
Reset clears root state while keeping that binding. There is no state factory on
BtNode and no update method on BtState.

## Deferred dynamic storage

Frame storage, runtime layouts, capacity hints, and backend selection are parked
until dynamic node boundaries exist. Their future responsibility is state whose
concrete type is not included in the surrounding static node type. They must not
replace direct typed access throughout a static subtree. The earlier stack API
and its implementation have been removed from this draft rather than retained as
an unused public contract.

The bt! compiler remains future work. The generic tuple implementation already
generates the corresponding state enum; future code generation must likewise
emit both definition composition and state composition.

## Validation scope

Focused tests cover resume, sequence/selector revalidation, failed-candidate
preservation, preemption, root binding, state cleanup, and persistent state size. Custom composing nodes
exercise direct nested-state access and destruction. The former backend test is
replaced by a static composition scenario that rejects a Running candidate and
then resumes the old branch without losing its progress.

The [action draft](action-draft.md) adds lifecycle callbacks inside ordinary
update without changing static composition. The post-commit target experiment
is archived separately; dynamic composition remains future work. User panics
are not caught; reset before reusing state after an unwind.

The [local state draft](local-state-draft.md) adds a separate parameter generic
and explicit bindings without changing the static ownership model.
