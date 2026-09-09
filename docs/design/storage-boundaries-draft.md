# Storage boundaries: first discussion draft

Status: deferred proposal; no storage implementation. Based on main at `d0ce88c`.
The archived experiment at `52e50b4` supplies evidence, not a required protocol.

The current direction is [choice from a statically known set](choose-draft.md).
The constraints below describe an earlier proposal, not requirements for that
work. Open runtime candidate sets and their memory model will be reconsidered
separately; heap allocation and non-linear storage may be acceptable there.

## Ownership does not require flattening

Proposed persistent representation:

```text
BtState
  borrowed root definition
  one inline storage backend
    root frame: Root::State
      policy state, static child enums, nested static state
      metadata for an active dynamic boundary, if any
    dynamic frame A: A::State
      static descendants, including a boundary to B
    dynamic frame B: B::State
      static descendants, including a boundary to C
    dynamic frame C: C::State
      static descendants
```

Storage owns the root value and every separately stored dynamic value. BtState
has no additional `Option<Root::State>`. Static fields remain owned transitively
by their containing Rust value; they need neither individual frames nor handles.
The generated child enum remains the source of static selection information.

Dynamic boundaries can nest to any depth that fits the configured storage and
execution resources. Each boundary enters another frame in the same buffer; it
does not create a new buffer or heap owner. Even adjacent dynamic nodes follow
this rule. A frame groups statically known state up to its dynamic boundaries,
not necessarily an entire selected path. With one retained path, root -> A -> B
-> C requires four frames, not a root frame and one universal dynamic slot.
Several simultaneously retained branches would require a separate composition
contract; nested dynamic nodes already belong in the first working draft.

This proposal interprets unified ownership as ownership of all *persistent*
invocation state. A fresh static candidate can still be an ordinary local value
during an update, as on main. Requiring storage to own even these temporary values
would be a separate change to candidate construction and typed composition.
Temporary dynamic payloads need update-local scratch from the selected backend,
shared across boundaries. Scratch is not another persistent owner in BtState.

## Inline capacity is a primary constraint

The target is no runtime-owned heap allocation when persistent state and temporary
work fit their configured inline capacities and alignment is supported. This
includes frame headers, boundary metadata, and update scratch, not only payloads.
User-owned action resources can allocate independently of the runtime.

For the nested path above, persistent capacity must cover Root::State, A::State,
B::State, and C::State together, including frame metadata and alignment padding.
Each static child enum still needs space only for its largest alternative plus
its tag/padding. Summing the dynamic frames does not require reserving every
possible dynamic alternative's payload.

Revalidation also needs simultaneous space for the saved path and fresh candidate
state. For example, replacing B -> C under a retained A can construct B' -> C'
while B -> C remains alive. A failed candidate releases B' and C'; an accepted
candidate replaces B and C while preserving Root and A. An outer rejection can
still discard the candidate before that replacement is finalized.

The memory budget must therefore cover the saved frames plus the peak temporary
candidate work, including ordinary call-stack state for static candidates. One
bounded scratch buffer belongs to the external update and serves all nested
boundaries. It must not be allocated anew per boundary or tracked by a heap-backed
frame table. Persistent and scratch capacities may be specified separately; their
exact construction API remains open. Moving accepted state must not allocate or
invalidate live typed borrows.

The first inline backend has no implicit heap fallback. Capacity/alignment errors
must produce a diagnostic and fail the affected entry. In particular, discovering
that a candidate does not fit must not first destroy the saved branch. Admission
to persistent storage, as well as scratch capacity, must be checked before an
irreversible replacement; fitting scratch alone is insufficient.

## Responsibilities

| Layer | Responsibility |
| --- | --- |
| Control/composing node | Select children, retain or reject candidates, end nested invocations. |
| Typed node | Update its concrete state and application context. |
| Frame-entry adapter | Connect a definition's state schema to a checked typed borrow. |
| Storage | Allocate, initialize, lend, transfer, and destroy frame values. |
| External update | Check the root binding and coordinate entry and cleanup. |

Storage initializes a frame when entry requests it and destroys it when its
lifetime ends. Within that frame, composing code still initializes and replaces
child fields using ordinary Rust operations. Storage need not understand those
fields. Their destructors run through normal assignment or destruction of the
containing value. Resource cancellation remains state-owned Drop behavior.

Storage records frame occupancy and layout; it does not choose children or infer
selection from traversal. Root occupancy can answer `BtState::is_running` between
updates without an additional saved running flag.

## Preserve the typed authoring path

Keep `BtNode<C>` and its associated State as the starting point. Within a static
frame, a parent should still be able to project an ordinary mutable reference:

```rust
self.child.update(&mut state.child, ctx, mode)
```

That call is the current static API, not a promise that dynamic composition will
require no additional execution access. A node entering separately stored state
must receive some restricted access to storage. Its shape remains open.

At a frame boundary, the adapter checks the schema and obtains `&mut N::State`.
A `DynBtNode<C>` protocol could expose schema and erased entry, with an automatic
adapter for typed nodes. A concrete root can use the same storage operations
without requiring virtual node dispatch. Do not introduce a separate projection
trait for plain fields and enum payloads without a demonstrated need.

The schema must support the actual backend operations: concrete type identity,
layout, initialization, and destruction. Transferring a live payload from scratch
must move ownership without duplicating it or destroying it twice. Schema equality
does not establish node identity: distinct definitions can use the same State.
The root binding and eventual dynamic definition binding remain separate concerns.

Typed borrows must prevent relocation, replacement, or destruction of the borrowed
frame. Entry into descendants must expose disjoint storage, not unrestricted
access to the whole owner. This borrowing requirement does not by itself dictate
the experiment's saved/scratch scope protocol or its public API.

## The unresolved boundary is branch lifetime

Main can drop a rejected static candidate and thereby release all its resources.
Once some descendants live in separate frames, dropping a marker inside that
candidate cannot alone release those frames: ordinary Drop has no mutable access
to the common storage owner.

The same issue occurs when a retained static branch is replaced by a purely
static candidate. The new candidate may never enter storage, yet the old branch's
dynamic payloads must be released. Moving the root into frame zero does not make
this relationship automatic.

Some explicit connection between composition and frame lifetime is therefore
needed. It could be restricted branch operations supplied to composers, or
storage-aware reconciliation of boundary metadata. The former affects composing
calls; the latter needs a way to inspect relevant nested state and establish
cleanup timing. Neither is free. A marker with its own Box/Rc/storage owner would
evade the common-owner requirement rather than solve this boundary.

Do not select the mechanism before demonstrating these observable behaviors:

1. A saved branch survives a failed fresh candidate with dynamic descendants.
2. An outer custom composer can reject a candidate whose nested child returned
   Running; the candidate's dynamic state is released and the saved branch survives.
3. Selecting a new static branch releases the old branch's dynamic descendants.
4. Completion, reset, and BtState destruction release each retained resource once.
5. A nested dynamic chain resumes through multiple frames in the common buffer;
   replacing its inner suffix preserves the retained outer states.
6. Insufficient candidate capacity preserves the saved nested branch, which can
   subsequently resume. Sufficient capacity requires no runtime heap allocation.

Returning Running from a nested child is not sufficient to commit globally; its
parent can still reject it. No new protocol should silently assume otherwise.
This concerns state lifetime, not rollback of context effects or selected-only
action tick. Those action semantics stay as on main.

## Small implementation steps to discuss

First, sketch the storage entry and branch lifetime contract against root -> A ->
B, with two nested dynamic boundaries and static state inside the frames. Review
how nested borrows, candidate rejection, and suffix replacement work before
generalizing composition access. A root-only wrapper would not validate these
boundaries and is insufficient as the first architectural proof.

Implement that small scenario with a bounded inline backend from the start. Check
resume, fresh Evaluate, inner replacement, outer rejection, terminal cleanup,
reset, and capacity failure. Verify the allocation target with allocation-free
application nodes, including nested revalidation. The first proof need not expose
a finalized public API or support every composition form.

A boxed backend can subsequently serve as a reference implementation of the same
contract. It is not the default design premise: boxing the root would introduce
allocation even for purely static trees. Keep owning heap allocations confined
to that backend. Do not impose a universal destruction order between separate
frames or create a frame for every static node.
