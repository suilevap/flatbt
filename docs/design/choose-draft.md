# Choice from a statically known set

Working draft based on main at `d0ce88c`. Review before committing.

Focus on code-authored trees whose candidate types are known to the compiler.
Open sets of runtime-defined node types and their storage are deferred. Their
eventual design may permit heap allocation or non-linear ownership; the earlier
[storage boundaries proposal](storage-boundaries-draft.md) is not a requirement
for this iteration.

## Authoring

```rust
let tree = choose!(|bb: &Blackboard| match bb.order {
    Order::Move => MoveNode::new(movement_config),
    Order::Attack => AttackNode::new(weapon_config),
    Order::Idle => IdleNode,
});
```

Unlike a Rust closure returning a node, this macro separates definitions from
selection. Every arm expression is evaluated once, in source order, when building
the tree. The resulting concrete definitions are stored in a tuple. A generated
closure matches shared context and returns the corresponding tuple index.
The compiler checks pattern exhaustiveness and the context type of every node.

Patterns, guards, and match bindings work in selection. Definitions cannot depend
on the context argument or those bindings: `MoveNode::new(bb.target)` would refer
to update-time data during construction. Read invocation inputs from the context
inside the node instead. `move |bb: &Blackboard| match ...` owns chooser captures;
the ordinary form borrows captures according to Rust's closure rules.

This first syntax requires a named, typed context argument. Separate expression
arms with commas; plain block arms can omit them. Branches can contain node
construction expressions, including
static compositions and nested `choose!` calls. There is no procedural macro or
additional dependency. The candidate limit is the existing tuple limit configured
by FLATBT_MAX_CHILDREN (default 32).

The build script supplies literal indices from that same limit to a hidden helper
macro. Expanded chooser arms therefore read `Order::Move => 0`, `Order::Attack =>
1`, and so on, rather than exposing token-counting expressions. There is no second
hand-maintained limit and no runtime counting operation.

## Selection and lifetime

ChooseNode is an alias for `ControlNode<Choose<F>, Children>`. Choose implements
BtControl with `State = ()`: begin requests the chosen child, and the two terminal
callbacks forward Success or Failure. ControlNode supplies the existing BtNode
implementation, including Resume/Evaluate handling and child-index validation.
The state is `ControlState<(), Children::State>`; there is no extra saved index,
type erasure, or frame owner. `ChooseNode::new(children, choose)` and the macro
syntax remain unchanged.

- Fresh entry evaluates the chooser and enters the selected child as Evaluate.
- Evaluate runs the chooser again. Selecting the same arm preserves its state;
  selecting another arm creates fresh candidate state.
- Resume follows the active enum variant without invoking the chooser.
- A Running candidate replaces the previous child state. A terminal result is
  returned to the parent without trying any other arm. As with other nodes, the
  invocation owner then drops this node's state, including any old child retained
  while the terminal candidate ran.
- Each arm has independent identity, even if its node type matches another arm.
  Use one arm with an or-pattern to share a candidate across several order values.

A parent can reject this entire node as a fresh candidate. Ordinary Rust Drop
then releases its nested states while the parent's saved branch stays intact.
Context mutations and action ticks are immediate, as on main; they are not rolled
back. A different candidate runs before the old child's state is dropped, using
the existing tuple dispatch semantics.

## Memory and nesting

The generated state enum contains Empty or one concrete candidate state. Persistent
space is approximately the largest candidate state plus tag/alignment, including
any nested choice enums. All static hierarchy remains intact. Candidate execution
uses temporary call-stack state before moving a Running value into the saved enum.

The selection/composition mechanism introduces no heap allocation. Definitions,
closure captures, or application-owned state can still allocate by their own
choice. No inline capacity setting is needed here: Rust computes the state layout.
This does not eliminate potentially large call-stack requirements or guarantee
immovable state.

Nested choices use exactly the same protocol as their parents. Changing an inner
choice under a retained outer arm preserves the outer state; switching the outer
arm drops its entire old hierarchy. There is no separate frame cleanup.

## Review evidence

`tests/choose.rs` covers saved choice on Resume, reselection and state reuse on
Evaluate, distinct same-type arms, nested replacement, outer rejection, terminal
result forwarding, cleanup, one-time construction, guards, and captured selection
data. `examples/choose.rs` demonstrates movement, a nested attack choice, and idle
using ordinary compositions. Utility behavior stays outside core.

## Direct node versus control policy

The first draft implemented BtNode directly. The current version removes that
implementation and delegates execution to ControlNode. The five choice behavior
tests pass unchanged, including nested selection and rejection. No change to the
macro expansion or the shared control implementation was needed.

| Aspect | Direct BtNode | ControlNode with Choose |
| --- | --- | --- |
| Execution rules | Local Resume/Evaluate handling | Shared control execution |
| Custom logic | Select and call the child | Select in begin; forward terminal results |
| Associated State | Children::State | ControlState<(), Children::State> |
| Public types | ChooseNode struct | Choose policy and ChooseNode alias |
| Implementation before macro docs | 29 lines | 34 lines |

The control version has slightly more policy boilerplate but a smaller semantic
responsibility: it only specifies the decision and terminal outcomes. Future
changes to control execution no longer need to be mirrored here. Choose can also
be used directly with `control(Choose(chooser), children)`.

A one-off comparison with rustc 1.98.1 on aarch64-apple-darwin used candidates
with `[u64; 1]` and `[u64; 32]` state. Both implementations had 264-byte state and
272-byte BtState; a nested choice had the same sizes in both implementations.
For the simple two-candidate update, optimized assembly had identical instructions
and called the same tuple dispatch function (only local labels differed). The
common control loop and terminal policy callbacks introduced no extra instructions
in that example. This is a layout/code-generation check, not a throughput benchmark
or a guarantee for every tree and compiler.

Prefer the control version for the current contract: it reuses the execution
protocol with no observed layout or code-generation penalty in the comparison.
The direct version remains shorter and exposes the child state without a wrapper,
but currently provides no distinct execution behavior that needs its own BtNode.
The associated State type has changed, even where its measured size is identical;
code that explicitly names or inspects the former child enum must account for the
ControlState wrapper. Low-level invalid indices now use the common control error
diagnostic; both variants fail without invoking an invalid child.
