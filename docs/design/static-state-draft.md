# Static state composition draft

This iteration is based on `3a09d11`, the last simple M1 implementation.
The earlier pool-based iteration remains on `backup/before-stack-draft`.
These notes describe the current draft for review, not a frozen architecture.

## Static definitions imply static state

A statically composed node knows the types of its children, including their
associated State types. Its own State must include those states. Routing every
static node through a heterogeneous frame stack lost that property and forced
execution to choose state that already had a known place in the parent.

`BtNode::update` now receives only its concrete state, context, and EntryMode.
A composing node borrows a field and calls the child directly:

```rust
self.child.update(&mut state.child, ctx, mode)
```

No cursor, storage type parameter, or Reuse/Fresh selector participates. A custom
node owns the lifecycle of its nested state: it initializes fresh invocations,
passes Evaluate on fresh entry, and drops completed or preempted invocations.

## Product layout for control nodes

The tuple implementation generates child dispatch and child state together:

```text
ControlNode<Policy, (A, B, C)>::State
  = ControlState<Policy::State,
      TupleState<(Option<A::State>, Option<B::State>, Option<C::State>)>>
```

ControlState contains the child states, policy state, and active child index.
Tuple dispatch selects both the definition and its state field in the same match
arm. The private run_node helper receives that exact typed Option by reference;
it initializes it on first entry and clears it on a terminal result. It performs
no lookup, allocation, type erasure, or continuation selection.

TupleState is a wrapper that supplies Default for tuples through arity 32;
initialization sets all slots to None without constructing their node states.
The complete layout is known to Rust. State size is the product layout's total,
including Option tags and padding; unused slots still reserve space. The runtime
adds no heap allocation. User-defined state can allocate its own resources.

This is a deliberate first representation. A generated sum/enum layout can reduce
reserved space later, but revalidation must allow an old branch and a candidate
to coexist until the selection is resolved. A single active enum variant alone
would not preserve that behavior without additional temporary storage.

## Selection and lifetime

Resume follows the control's active child. Evaluate asks the policy to choose
again: Sequence preserves its active index; Selector begins at zero. An existing
state field receives the requested mode; a newly initialized field gets Evaluate.
Presence of state does not force Resume.

During revalidation, the old branch remains in its own field while another field
holds the candidate. A terminal candidate is dropped without disturbing the old
field or index. A Running candidate causes ControlNode to reset the old field and
record the new active index. No saved/scratch stack pair is needed.

When the parent invocation terminates, its state is dropped, including any
remaining nested state. ControlState declares children before policy state so
normal destruction releases descendants first. Custom composing states own their
field ordering and cleanup. Context mutations are never rolled back.

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
generates the corresponding state product; future code generation must likewise
emit both definition composition and state composition.

## Validation scope

Focused tests cover resume, sequence/selector revalidation, failed-candidate
preservation, preemption, root binding, and state cleanup. Custom composing nodes
exercise direct nested-state access and destruction. The former backend test is
replaced by a static composition scenario that rejects a Running candidate and
then resumes the old branch without losing its progress.

Post-commit tick handling and dynamic composition remain unimplemented. User
panics are not caught; reset before reusing state after an unwind.
