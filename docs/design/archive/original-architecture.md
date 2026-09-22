# Original architecture proposal

Historical design, translated from the Russian source. This records the original
v1 plan, including unimplemented and superseded proposals. For current behavior,
see the [README](../../README.md) and [decision log](decisions.md).
Signatures and snippets below are conceptual.

## 1. Core idea

A behavior tree is a resumable program. Two APIs share its execution semantics.

### Low-level runtime API

Typed Rust combinators use generic composition and static dispatch:

```rust
select((
    seq((check1(), a(), b())),
    seq((check2(), c(), d(), e())),
    seq((fallback(), wait(5.0))),
))
```

This permanent reference API validates runtime semantics before the DSL compiler.
Statically known children require no `dyn` dispatch.

### Compiled API

```rust
bt! {
    select {
        {
            check(ctx.can_attack());
            attack();
        }
        {
            move_to_target();
            wait(0.5);
        }
    }
}
```

The original v1 plan includes `bt!`, implemented after runtime semantics stabilize.
It can flatten control flow, inline synchronous conditions, omit trivial nodes,
and persist only locals that survive suspension.

## 2. `BtNode`

Lowest-level execution protocol; exact signatures and lifetimes remain open.

```rust
enum EntryMode {
    Resume,
    Evaluate,
}

trait BtNode<C> {
    type State: Send + 'static;

    fn update(
        &self,
        state: &mut Self::State,
        ctx: &mut C,
        exec: &mut ExecutionCursor,
        mode: EntryMode,
    ) -> NodeResult;
}
```

| Entry mode | Behavior |
| --- | --- |
| `Resume` | Follow the saved continuation of the selected invocation. |
| `Evaluate` | Re-run its decision logic. Preserve existing state. |

On Evaluate, Sequence continues its active child, reactive Selector scans from
child zero, and DynamicOrder rereads the current order. Fresh invocations always
enter as Evaluate. Nodes unaffected by revalidation may ignore the mode.

## 3. Revalidation

Revalidation belongs to execution. The evaluator selects the policy; execution
computes each node's `EntryMode`. Nodes do not query storage for the policy.

Initial policies: `ResumeCurrentPath` and `ReevaluateFromRoot`. Intermediate
checkpoints may follow.

Example saved path and normal continuation:

```text
Root          Resume
  Combat      Resume
    DynamicOrder  Resume
      Sequence    Resume
        Move      Resume/Evaluate, according to suspension semantics
```

Full reevaluation starts at Root with Evaluate.

## 4. Execution and storage

| Layer | Responsibilities |
| --- | --- |
| `ExecutionCursor` | Traverse saved paths; track revalidation boundaries; distinguish fresh and existing invocations; choose entry modes; evaluate candidates; change continuations; coordinate commit/rollback. |
| `BtState`, `FrameStorage`, `ThinkScratch` | Own frames; provide typed access, allocation, alignment, destruction, persistent/scratch memory, physical commit, and relocation. |

Dependency: `ExecutionCursor → FrameStorage`. Storage has no Sequence/Selector,
entry-mode, revalidation-boundary, or full-reevaluation semantics.

## 5. Static dispatch

Concrete parents call concrete children. Statically known children must not
silently become `&dyn BtNode`:

```rust
match child_index {
    0 => run_child_static(&self.children.0, ...),
    1 => run_child_static(&self.children.1, ...),
    2 => run_child_static(&self.children.2, ...),
    _ => unreachable!(),
}
```

Generic helpers such as `run_child_static<N: BtNode<C>>` are monomorphized.
Reserve `DynBtNode` for explicit runtime-dynamic boundaries.

## 6. Tuple children

Represent heterogeneous children with ordinary Rust tuples, e.g. `(Check1, A, B)`.
No custom HList. Without variadic generics, generate tuple implementations once
in the library, for example for arities 1–32.

```rust
trait BtChildren<C> {
    const LEN: usize;

    fn run_child(&self, index: usize, ...) -> NodeResult;
}
```

Each implementation matches the index and calls the concrete tuple field.
This machinery is shared across trees.

## 7. `ControlNode`

```rust
struct ControlNode<P, Children> {
    policy: P,
    children: Children,
}
```

```rust
seq((a(), b(), c()))
select((branch1, branch2, branch3))
```

Rust infers nested types such as `ControlNode<Selector, (ControlNode<Sequence,
(...)>, ...)>`; users need not write them.

## 8. `BtControl`

Compile-time control-flow policy. Returns logical child indices; `ControlNode`
performs static dispatch. Custom policies must preserve static dispatch.

```rust
trait BtControl<C> {
    type State: Default + Send + 'static;
    type ChildMeta;

    fn begin(...) -> ControlOp;
    fn child_succeeded(...) -> ControlOp;
    fn child_failed(...) -> ControlOp;
    fn child_committed(...) {}
}

enum ControlOp {
    RunChild(usize),
    Success,
    Failure,
}
```

## 9. `EntryMode` and `BtControl`

`EntryMode` belongs to `BtNode`. In `ControlNode::update`:

- Resume follows the framework-owned active child without calling `begin()`.
- Evaluate calls `begin()` to revalidate the decision.

Calling `begin()` supplies the revalidation signal; the policy needs no mode.

## 10. Control state

```rust
struct ControlState<S> {
    inner: S,
    active_child: Option<usize>,
}
```

The framework writes `active_child`. The policy owns `inner` and may read
continuation metadata.

## 11. Sequence

Memoryful: fresh entry starts at child zero; existing entry keeps the active
child, including on Evaluate.

```rust
RunChild(state.active_child.unwrap_or(0))
```

| Child result | Sequence response |
| --- | --- |
| Success | Run the next child. |
| Failure | Return Failure. |
| Running | Save the active child. |

A suspended child may finish and the next child run in the same external update.

## 12. Reactive Selector

Resume follows the active child without `begin()`. Evaluate starts at child zero.

| Child result | Selector response |
| --- | --- |
| Success | Return Success. |
| Failure | Try the next child. |
| Running | Save the active child. |

Reevaluation can try child zero, receive Failure, then Evaluate the old Running
child one with its saved state. Existing state and entry mode are independent.

## 13. Invocation identity

Valid combinations:

- Existing frame + Resume.
- Existing frame + Evaluate.
- Fresh frame + Evaluate.

A terminal result ends the invocation. Selecting the same index again starts a
fresh invocation, even within the same external update.

## 14. DynamicOrder

Example custom `BtNode` with runtime selection:

```rust
struct DynamicOrderState {
    selected: BehaviorHandle,
}
```

Resume uses `state.selected`; Evaluate rereads the order from context/blackboard.
The saved selection needs owned, stable identity: `Arc`, asset/behavior handle,
or stable ID with compatible lookup.

Invariant: saved state must never reach an incompatible dynamic definition.

## 15. Suspension

Any `BtNode` may return Running across several external updates, then complete.
No action or tick capability is required. All entries use `update(..., EntryMode)`;
there is no separate `resume()` method.

## 16. `BtTick`

Optional post-commit execution:

```rust
trait BtTick<C>: BtNode<C> {
    fn tick(&self, state: &mut Self::State, ctx: &mut C);
}
```

Running suspends an invocation. `BtTick` requests work after the resulting Running
continuation has been selected. Only `tick()` has that guarantee.

## 17. Speculative side effects

`BtNode::update` and action `start`, `is_in_progress`, and `complete` may run on
candidates later rejected. Irreversible gameplay effects that depend on final
branch selection belong in `BtTick::tick`.

## 18. `NodeResult`

```rust
enum NodeResult {
    Success,
    Failure,
    Running {
        tick: Option<ActiveRef>,
    },
}
```

Representation remains open. Running need not include a tick target.

## 19. `ActiveRef`

Transient lifetime: update result → logical commit → immediate tick.
Resumption does not depend on it.

Only the framework may construct a target for the current concrete frame,
pairing `N` with `N::State` where `N: BtTick<C>`. Safe user code must not pair a
node with another node type's state.

## 20. `BtAction`

```rust
trait BtAction<C> {
    type State: Send + 'static;

    fn start(&self, ctx: &mut C) -> Option<Self::State>;
    fn is_in_progress(&self, state: &Self::State, ctx: &C) -> bool;
    fn tick(&self, state: &mut Self::State, ctx: &mut C);
    fn complete(&self, state: &Self::State, ctx: &mut C) -> bool {
        true
    }
}

struct ActionNodeState<S> {
    action: Option<S>,
}
```

```text
start → None         → Failure
start → Some(state)  → is_in_progress
    true             → Running + tick target
    false            → complete immediately → Success/Failure
```

No PendingComplete phase.

## 21. Runtime state

Definitions hold immutable code/configuration and may be shared across agents.
Each agent owns mutable invocation state for its current execution path only.

## 22. Threading

Frame state requires `Send + 'static`, not `Sync`. Each `BtState` is accessed
exclusively but may move between worker threads between updates.

Target: `BtState: Send + !Sync`. Marker implementation remains open. Bevy may use
an exclusive-access wrapper.

## 23. Prototype storage

One Box per active frame:

- Stable addresses; descendant allocation preserves parent borrows.
- Allocator handles alignment.
- Arbitrary owning state and normal Drop.
- Simple execution-semantics validation.

Keep as a possible reference/debug backend for differential testing.

## 24. Production storage target

After semantics stabilize: inline persistent capacity + stable overflow segments
+ reusable scratch.

Requirements:

- Frames stay in place during traversal.
- `BtState` may move between external updates.
- Logical `StateOffset`; no persistent raw pointers.
- Correct alignment and arbitrary `Send + 'static` state.
- Relocation only at quiescent points.
- Standalone destruction.

## 25. Drop infrastructure

Each owning storage keeps sparse destructor metadata for `needs_drop::<S>()` frames:

```rust
struct DropEntry {
    state: StateOffset,
    drop_fn: DropThunk,
}
```

Support reverse-order destruction, scratch rollback, persistent suffix discard,
exactly-once ownership transfer on commit, ZSTs with Drop, and standalone
`BtState::drop`. Memory safety must not depend on semantic cancellation traversal.

## 26. Logical and physical commit

| Phase | Contract |
| --- | --- |
| Logical commit | Select the resulting Running continuation. No sibling may replace it in this traversal. Call `child_committed` here. |
| Physical commit | After frame borrows unwind, discard the old divergent suffix, move candidate state and DropEntry ownership, then compact/relocate if needed. |

Keep these phases separate.

## 27. Cancellation

No native cancel/abort hooks in v1. Preemption guarantees normal Rust Drop of
discarded state. Gameplay cleanup uses tick-confirmed effects, RAII, leases,
reconciliation, and game-level ownership.

## 28. Unsafe boundary

Confine unsafe code to framework storage, alignment, logical-to-physical address
resolution, typed reconstruction, relocation, drop thunks, and necessary erased
dynamic adapters.

Keep `BtNode`, `BtTick`, `BtAction`, `BtControl`, combinators, and `bt!` safe.

## 29. `bt!` compiler frontend

Part of the original v1 plan; implemented after the runtime/combinator backend.
Compile the DSL to resumable code without requiring a node-for-node graph.

## 30. Standard control-flow flattening

Compiler primitive: `compile(node, on_success, on_failure)`.

For children A, B, C:

| Node/result | Sequence edge | Selector edge |
| --- | --- | --- |
| A.Success | B | Selector.Success |
| A.Failure | Sequence.Failure | B |
| B.Success | C | Selector.Success |
| B.Failure | Sequence.Failure | C |
| C.Success | Sequence.Success | Selector.Success |
| C.Failure | Sequence.Failure | Selector.Failure |

Running saves a resume label and returns Running.

## 31. Generated state machine

```rust
enum Pc {
    Start,
    A,
    B,
    C,
    D,
    Wait,
}
```

Generated execution loops over `match pc`. Resume loads the saved PC and jumps
to the suspended point, avoiding traversal through static ancestors. Sequence
and Selector frames may disappear because the PC encodes their continuation.

## 32. Why explicit flattening

Inlining nested generic calls does not necessarily combine persistent
`Selector.active_child` and `Sequence.active_child` into one PC across updates.
Explicit CFG lowering may help. Benchmark the generic backend first.

## 33. Inline synchronous code

```rust
bt! {
    check(ctx.has_target());
    shoot();
}
```

May lower directly to:

```text
if !ctx.has_target(): goto failure
run_shoot()
```

The check needs no `Check<Closure>`, node frame, or node state.

## 34. Locals

```rust
bt! {
    let distance = ctx.distance();
    check(distance < 10.0);
    shoot();
}
```

A local unused after suspension stays on the Rust stack; no persistent field.

## 35. Locals across suspension

```rust
bt! {
    let target: Entity = ctx.best_target();
    move_to(target); // May return Running.
    shoot(target);
}
```

`target` survives suspension and becomes a generated state field:

```rust
struct GeneratedState {
    pc: Pc,
    target: Entity,
}
```

v1 may require explicit types for persistent locals: proc macros lack rustc's
full type inference for generated fields.

## 36. DSL scope

Controlled grammar with BT-specific suspension semantics.

| In v1 | Outside v1 |
| --- | --- |
| Sequence, Selector, BtNode calls, static dispatch, flat CFG | Arbitrary loops with suspension |
| Inline conditions, explicit success/failure, simple `if`, synchronous expressions | Arbitrary `match` with complex persistent bindings |
| Stack locals and typed persistent locals | Borrowed locals across suspension, iterator lowering |
| Explicit persistent field types | Full Rust coroutine semantics or automatic field-type inference |

## 37. Custom controls and flattening

Standard Sequence/Selector regions may flatten around custom control nodes:

```text
flattened standard region
    → CustomControlNode
        → flattened standard child region
```

Custom `BtControl` is an initial optimization boundary. Its heterogeneous static
children still use static dispatch. An opt-in lowering protocol may follow v1.

## 38. Dynamic boundaries in `bt!`

```text
flattened static code → DynamicOrder / DynBtNode → runtime-selected behavior
```

Opaque runtime behavior need not flatten.

## 39. Milestones

Each milestone ends with an executable BT example.

| Milestone | Implementation | Deliverable |
| --- | --- | --- |
| M0: synchronous runtime | BtNode, Success/Failure, BtControl, ControlNode, tuple children, Sequence, Selector, custom policies, static dispatch. No Running/state stack or `bt!`. | Execute nested `select((seq(...), seq(...)))`. |
| M1: suspension | Persistent state, EntryMode, ResumeCurrentPath, boxed frames. | `check → wait_frames(3) → fire` across updates. |
| M2: revalidation | ReevaluateFromRoot, reactive selector reuse, losing candidates, preemption, existing state + Evaluate. | Priority changes preempt the old behavior. |
| M3: BtTick | Post-commit execution. | Losing candidates never tick; committed Running node ticks once per external update. |
| M4: BtAction | Lifecycle adapter. | Multi-frame Move/Aim/Fire/Wait AI. |
| M5: dynamic composition | DynBtNode, BehaviorHandle, DynamicOrder. | Switch runtime orders while static subtrees retain static dispatch. |
| M6: production storage | Inline storage, stable overflow, scratch, offsets, DropEntry, rollback, suffix discard, physical commit, ZST/alignment support, panic guards. | M1–M5 behavior tests unchanged. |
| M7: ECS/Bevy | Movable BtState, Send + !Sync, ECS wrapper. | Archetype relocation and despawn/destruction tests. |
| M8: compiler frontend | `bt!`, still in v1. | Stages below. |

M8 stages:

1. Basic DSL: Sequence, Selector, BtNode calls, success/failure/check. Initial
   lowering may follow generic semantics.
2. Flatten standard controls to a state machine; benchmark against the generic backend.
3. Inline checks and synchronous expressions; omit unnecessary wrapper nodes.
4. Support ordinary stack locals.
5. Persist typed locals live across suspension; no full Rust coroutine lowering.

## 40. Critical tests

1. Plain BtNode may suspend without Tick.
2. Fresh invocation enters as Evaluate.
3. Sequence preserves active child across revalidation.
4. Reactive Selector Resume follows active child.
5. Reactive Selector Evaluate restarts priority scanning.
6. Existing invocation may receive Evaluate.
7. DynamicOrder Resume uses saved selection.
8. DynamicOrder Evaluate rereads its source.
9. A suspended child completes and the next child runs in the same update.
10. A higher-priority candidate preempts the old branch.
11. Losing speculative branches never tick.
12. Tick occurs after logical commit.
13. Repeating a child after a terminal result starts a fresh invocation.
14. `child_committed` occurs only on an actual policy commit.
15. Static tuple children use no virtual dispatch.
16. Custom BtControl remains statically dispatched.
17. Standalone BtState Drop works.
18. Scratch rollback drops exactly once.
19. Commit transfers ownership exactly once.
20. ZST custom Drop works.
21. Parent references survive descendant storage growth.
22. Dynamic state never reaches an incompatible definition.
23. BtState may move between external updates.
24. BtState is Send but not Sync.
25. Generic and compiled trees have equivalent observable behavior.
26. Flattened Resume avoids walking all standard static ancestors.
27. Inline check creates no runtime node/frame.
28. Non-persistent locals stay on the stack.
29. Typed locals survive suspension.

## 41. Open implementation questions

- BtNode signatures/lifetimes; EntryMode representation; separate restricted
  Resume/Evaluate execution interfaces; ExecutionCursor and child-entry APIs.
- Definition/state schema binding; boxed frames; StateOffset encoding; inline
  capacity; overflow layout; scratch API; logical/physical commit algorithm.
- ActiveRef and DynBtNode representation; panic guards; !Sync marker; Bevy wrapper.
- Tuple arity limit; DSL grammar; generated CFG; persistent-local syntax/types;
  benchmark threshold for compiled versus generic execution.

## 42. Original design commitments

- One `BtNode::update(..., EntryMode)` method.
- Separate physical storage from execution and revalidation.
- Permanent generic reference API: `ControlNode<P, TupleChildren>`.
- `bt!` in v1, implemented last as a resumable compiler frontend.
- Validate semantics with boxed storage, then optimize storage, then add compiled lowering.
