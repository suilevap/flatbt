# Storage boundaries

Deferred proposal; no implementation. Based on `d0ce88c`; archived experiment
`52e50b4` supplied evidence. Current [static choice](../choose.md) does not
require this proposal. Open runtime sets may use heap allocation or non-linear
storage; their memory model remains open.

## Proposed ownership

```text
BtState
  borrowed root definition
  one inline storage backend
    Root::State: policy, static child enums, boundary metadata for A
    A::State: static descendants, boundary metadata for B
    B::State: static descendants, boundary metadata for C
    C::State: static descendants
```

Storage owns root and dynamic frames. No additional `Option<Root::State>` in
BtState. Static descendants remain ordinary fields, owned transitively; their
enum variants encode selection. No separate static frames or handles.

Each dynamic boundary enters another frame in the same buffer. Root → A → B → C
needs four frames, including adjacent dynamic nodes. Nesting is limited by storage
and execution resources. Multiple retained branches need another composition contract.

Unified ownership covers persistent state. Fresh static candidates may remain local
variables. Dynamic candidates use one update-local scratch area shared across
boundaries, separate from persistent ownership.

## Capacity and admission

Target: no runtime heap allocation when persistent state, frame metadata, padding,
and scratch fit configured inline capacities/alignment. User resources may allocate.

Persistent capacity covers every frame on the retained path. Each static enum
reserves its largest alternative plus tag/padding, not all alternatives.
Revalidation also needs peak candidate space: replacing B → C under A constructs
B' → C' while the saved path remains alive. Failure drops the candidate; acceptance
replaces the suffix and preserves Root/A. An outer parent can still reject it.

Include static call-stack candidates in the total memory budget. Use one bounded
scratch buffer per external update; no per-boundary buffer allocation or heap frame
table. Persistent/scratch capacities may be separate; construction API remains open.
Moving accepted state must neither allocate nor invalidate typed borrows.

The proposed first backend has no implicit heap fallback. Capacity/alignment failure
logs a diagnostic and fails entry while preserving the saved path. Check persistent
admission before replacement; fitting scratch alone is insufficient.

## Responsibilities

| Layer | Responsibility |
| --- | --- |
| Control/composer | Select children; retain/reject candidates; end nested invocations. |
| Typed node | Update concrete state and context. |
| Frame adapter | Validate schema; obtain a checked typed borrow. |
| Storage | Allocate, initialize, lend, transfer, destroy frames. |
| External update | Check root binding; coordinate entry and cleanup. |

Composers manage static fields through normal Rust initialization, assignment, and
Drop. Storage records occupancy/layout, without choosing children. Root occupancy
can answer `is_running` without another flag. Cancellation remains state-owned.

## Typed access and identity

Keep `BtNode` and direct static projection:

```rust
self.child.update(&mut state.child, ctx, params, mode)
```

Dynamic entry needs restricted storage access; API unresolved. An erased DynBtNode
could expose schema and entry with automatic typed adapters. Concrete roots can
use those storage operations without virtual dispatch. Plain fields and enum
payloads need no extra projection trait without evidence.

Schemas cover type identity, layout, initialization, and destruction. Moving scratch
payloads must transfer ownership exactly once. Equal state schemas do not establish
node identity; root and dynamic definition bindings are separate checks.

Typed borrows must prevent frame relocation, replacement, and destruction. Descendant
entry needs disjoint storage access rather than unrestricted access to the owner.
This does not require adopting the archived experiment's public protocol.

## Open issue: branch lifetime

Dropping a static candidate cannot automatically free separately stored descendants:
its marker has no mutable access to the common storage owner. Replacing a branch
with a purely static candidate has the same problem, even if the replacement never
enters storage. Putting the root in frame zero does not solve cleanup.

Possible mechanisms: restricted branch operations for composers, or reconciliation
of boundary metadata. The first changes composing calls; the second needs state
inspection and defined cleanup timing. A marker owning Box/Rc/another backend
would violate the proposed common-owner contract.

Validate before choosing a mechanism:

1. Failed dynamic candidates preserve the saved branch.
2. An outer composer can reject nested Running state, release the candidate's
   dynamic frames, and preserve the saved branch.
3. Static replacements release old dynamic descendants.
4. Completion, reset, and Drop release each resource once.
5. Nested dynamic resume crosses multiple frames; inner replacement preserves
   retained outer state.
6. Capacity failure preserves resumable saved state. Sufficient capacity requires
   no runtime allocation.

Nested Running does not commit globally. These requirements concern state lifetime;
context effects and inline action ticks keep their current semantics.

## First proof

Sketch Root → A → B with two dynamic boundaries and static state inside frames.
Resolve nested borrows, rejection, and suffix replacement before generalizing access.
A root-only wrapper cannot validate this boundary.

Implement the scenario with bounded inline storage. Check resume, fresh Evaluate,
inner replacement, outer rejection, completion, reset, and capacity failure. Measure
allocations using allocation-free application nodes, including nested revalidation.
No final public API or universal composition support is needed for this proof.

A boxed reference backend may follow under the same contract. Keep heap ownership
inside that backend. Avoid one frame per static node or a universal destruction
order across separate frames.
