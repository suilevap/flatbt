# Choice from a statically known set

Implemented draft. Open sets of runtime-defined node types and their storage are
[deferred](archive/storage-boundaries.md).

## Syntax

```rust
let tree = choose!(|bb: &Blackboard| match bb.order {
    Order::Move => MoveNode::new(movement_config),
    Order::Attack => AttackNode::new(weapon_config),
    Order::Idle => IdleNode,
});
```

- Every arm definition is constructed once, in source order, and stored in a tuple.
- Selection matches shared context and returns a generated tuple index.
- Rust checks exhaustiveness and every node's context type. Patterns, guards, and
  bindings work in selection. Definitions cannot reference `bb` or match bindings.
- Use `move |bb: &Blackboard| match ...` to own chooser captures. Otherwise normal
  Rust closure borrowing applies.
- The context argument must be named and typed. Expression arms need commas;
  plain block arms may omit them. Arms can contain compositions and nested choices.
- `FLATBT_MAX_CHILDREN` limits candidates (default 32). Core's build script supplies
  literal indices to the macro. No separate limit, runtime counting, proc macro,
  or external dependency.

## Selection and lifetime

`ChooseNode` wraps `ControlNode<Choose<F>, Children>` and delegates execution.
`Choose` uses `State = ()`: begin selects a child; terminal callbacks forward its
result. State is `ControlState<(), Children::State>` with no extra saved index.

| Entry/result | Behavior |
| --- | --- |
| Fresh entry | Run chooser; enter selected child with Evaluate. |
| Evaluate, same arm | Run chooser again; reuse saved child state. |
| Evaluate, different arm | Construct fresh candidate state while old state remains alive. |
| Resume | Follow saved variant without calling chooser. |
| Running candidate | Replace and drop old child state. |
| Terminal result | Return directly, without fallback. Invocation owner drops remaining state. |

Each arm has distinct identity, even when types match. Use an or-pattern in one
arm to share a candidate across order values. Rejecting an outer candidate drops
its nested state while preserving the parent's saved branch. Context writes and
inline action ticks survive rejection.

## Memory and nesting

The generated enum stores Empty or one candidate's state. Persistent size is
approximately the largest alternative plus tag/alignment. Candidate evaluation
uses call-stack space; selection moves state into the saved enum. Resume borrows
it in place. Large states or deep nesting can need substantial stack space.

Choice adds no heap allocation or type erasure. Definitions, captures, and user
state may allocate. Rust computes the inline layout; no capacity setting.
Changing an inner choice preserves the retained outer arm. Replacing an outer arm
drops its entire old hierarchy.

## API and implementation history

Use `ChooseNode::new(children, chooser)` for manual indices, or
`control(Choose(chooser), children)` for the policy API. Invalid indices use the
common control diagnostic and return Failure.

The first implementation called children directly from BtNode. Delegation moved
Resume/Evaluate and cleanup into the shared control implementation. It changed
associated state from `Children::State` to `ControlState<(), Children::State>`.
The later [crate split](package-layout.md) replaced the ChooseNode alias with
a wrapper so its constructor could stay in the catalog crate.

A historical comparison on rustc 1.98.1, aarch64-apple-darwin, used `[u64; 1]` and
`[u64; 32]` candidate states. Direct and control implementations both had 264-byte
state and 272-byte BtState, including the tested nested choice. A two-candidate
optimized update had identical instructions except local labels. This was a
layout/code-generation check, not a throughput or cross-compiler guarantee.

## Validation

[Tests](../../tests/choose.rs): Resume selection, Evaluate reuse/reselection,
same-type arm identity, nesting, rejection, terminal forwarding, cleanup,
one-time construction, guards, and captures. The five behavior tests passed
unchanged after delegation to ControlNode.

[Example](../../examples/choose.rs): movement, nested attack choice, idle.
