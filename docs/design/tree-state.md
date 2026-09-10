# Tree and state

One shared definition, inline state per agent. Static dispatch, nested Rust
values, no runtime frames.

## Code

```rust,ignore
select((seq((check(can_move), Move)), Idle))
```

```text
MoveBranch = ControlNode<Sequence, (Check<F>, Move)>
Tree = ControlNode<Selector, (MoveBranch, Idle)>
```

`F` = type of `can_move`. `MoveBranch` and `Tree` name concrete node types below.
`Sequence` and `Selector` are policies; their `BtControl::State` is `()`.
Node state is `ControlState<Policy::State, Children::State>`.

## Graph

Familiar node-link view of the same tree:

```mermaid
%%{init: {'flowchart': {'wrappingWidth': 1000}}}%%
flowchart TB
    gsel["select"] --> gseq["sequence"] & gidl["Idle"]
    gseq --> gchk["check"] & gmov["Move"]
```

## Definition vs state

Solid arrows expand inline fields; dashed arrows borrow the shared definition.
Labels are schematic. Rust chooses field offsets and enum layout.

```mermaid
%%{init: {'flowchart': {'wrappingWidth': 1000}}}%%
flowchart LR
    subgraph TREE["DEFINITION · BUILT ONCE · Tree"]
        direction TB
        tpol["Tree = ControlNode {<br/>#160;#160;policy: Selector, // child-selection rule<br/>#160;#160;children: (<br/>#160;#160;#160;#160;MoveBranch,<br/>#160;#160;#160;#160;Idle<br/>#160;#160;)<br/>}"]
        tseq["children.0 = MoveBranch {<br/>#160;#160;policy: Sequence, // child-selection rule<br/>#160;#160;children: (<br/>#160;#160;#160;#160;Check&lt;F&gt;,<br/>#160;#160;#160;#160;Move<br/>#160;#160;)<br/>}<br/>// predicate + Move config here"]
        tidl["children.1 = Idle"]
        tpol --> tseq & tidl
        tina["Inactive branches included"]
    end
    subgraph AA["AGENT A · RUNNING MOVE · BtState"]
        direction TB
        aref["root_node: &amp;Tree"]
        ast["root_state: Some(<br/>#160;#160;ControlState {<br/>#160;#160;#160;#160;inner: (),<br/>#160;#160;#160;#160;children: Child0(..)<br/>#160;#160;}<br/>)"]
        aseq["MoveBranch::State = ControlState {<br/>#160;#160;inner: (), // = policy state<br/>#160;#160;children: Child1(<br/>#160;#160;#160;#160;Move::State // owned data<br/>#160;#160;)<br/>}"]
        ast --> aseq
        ano["Check, Idle: no live state here"]
    end
    subgraph AB["AGENT B · BtState"]
        direction TB
        bst["root_node: &amp;Tree<br/>root_state: Some(<br/>#160;#160;ControlState {<br/>#160;#160;#160;#160;inner: (),<br/>#160;#160;#160;#160;children: Child1(Idle::State)<br/>#160;#160;}<br/>)"]
        bno["A runs Move, B runs Idle"]
    end
    classDef mono font-family:monospace,text-align:left
    class tpol,tseq,tidl,aref,ast,aseq,bst mono
    aref -. borrow .-> tpol
    bst -. borrow .-> tpol
```

Agent A: root child enum holds `Child0(MoveBranch::State)`; the nested enum
holds `Child1(Move::State)`. Agent B: root child enum holds `Child1(Idle::State)`.
Each agent owns a complete `Option<Tree::State>` and borrows the same definition.

## Layout

- Structs/enums nested inline, one value; no flat array or per-node pointers.
- Child enum: `Empty | Child0(S0) | Child1(S1)`.
- Child size ≈ max(S0, S1) + tag/align.

## During update

- Resume: borrow saved in place. Fresh candidate: temp stack.
- Fresh Running candidate → replace old. Terminal candidate → drop, preserve old.
- Terminal active child → Empty. Terminal root → None.
- User state may heap-alloc.
