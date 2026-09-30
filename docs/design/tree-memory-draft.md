# Tree memory: node state that outlives a run

Status: proposal. Core change; breaks every `BtNode` implementation once.

## Problem

A node's `State` is invocation state: created when the node starts, dropped
when it ends, and a control keeps only its active child's. Nothing a node knows
survives its own completion, so anything that must -- when a cooldown last
fired, whose turn a round-robin is on -- has had to live in the blackboard,
through an accessor the author adds per use. The blackboard is the world's view
of the agent; the tree should read it, not keep its own bookkeeping there. In
Bevy the tree does not write it at all, and `gather` rebuilds it every frame.

## Proposal

A second kind of per-agent node state, laid out statically like the first and
owned next to it:

| | Run state (`State`, today) | Memory (`Memory`, new) |
| --- | --- | --- |
| Lives | While the node runs | As long as the agent's `BtState` or `Behavior` |
| Kept for | The active child only | Every node, active or not |
| Layout | One variant per control: the largest branch | A struct of all children: the sum |
| Reset | Each time the node ends | Only by `forget()` |
| Examples | Deadlines, counters, the running child | Cooldowns; later round-robin, shuffle bag, "once" |

```rust
cooldown(Duration::from_secs(2), action(Dash))            // from each try
success_cooldown(Duration::from_secs(10), action(Heal))   // from each success
```

No blackboard field, no accessor: each node's memory sits at its position in
the tree, so two cooldowns never share a slot by accident, and one tree
definition still serves many agents, each with its own memory.

## Contract

```rust
pub trait BtNode<C, A = (), P = ()> {
    type State: Default + Send + 'static;
    type Memory: Default + Send + 'static;   // new
    const NODES: usize = 1;

    fn update(
        &self,
        state: &mut Self::State,
        memory: &mut Self::Memory,           // new
        ctx: &mut C,
        params: P,
        entry: Entry<'_>,
    ) -> NodeResult<A>;

    fn inspect(
        &self,
        state: Option<&Self::State>,
        memory: &Self::Memory,               // new: always present
        inspector: &mut dyn Inspector,
    ) { .. }
}
```

- **A node without memory** says `type Memory = ();` and ignores the argument.
  Associated types cannot have defaults on stable Rust, so this line is
  required in every implementation; see *Alternatives*.
- **Composers pass each child its own memory**, always the same place: memory
  is not candidate-local. A composer's `Memory` is its own plus its children's.
- **Memory writes are effects.** A candidate that writes memory and is then
  rejected keeps the write, as context writes and scope outputs already do. A
  cooldown in a branch that starts and fails the same update is spent.
- **Memory never resets on its own.** Not on completion, `Evaluate`, preemption
  or `reset()`.

### Composition

- `BtChildren` gains `type Memory` -- a tuple of the children's memories, all
  of them -- and `run_from` and `inspect_children` take it. Child `i` receives
  `&mut memory.i`. Generated in `build.rs` with the rest.
- `ControlNode::Memory` is `(P::Memory, Children::Memory)`: `BtControl` gains
  `type Memory` and its callbacks receive it, so a policy such as round-robin
  can remember across runs. `BtOrder` likewise, for a shuffle bag.
- Wrappers (`guard`, `scope`, `bind`, `order_by`, the decorators) forward their
  child's memory, adding their own when they need it.
- `BtAction` is unchanged: its adapter's memory is `()`. An action wanting
  memory is wrapped in a node that has it.

Changing `BtControl` and `BtOrder` in the same step breaks their
implementations once rather than twice.

### Size

Memory is laid out as a sum, not a maximum, but only nodes that use it take
space: `()` and tuples of `()` are zero-sized, so a tree without memory nodes
has zero-sized memory and the same run state as today. A test pins that. A
cooldown costs one `Option<Time>` per node per agent.

### Roots

| API | Change |
| --- | --- |
| `BtState` | Holds `memory: N::Memory` beside the run state. |
| `BtState::reset` | Unchanged: drops the run state, keeps memory. |
| `BtState::forget` | New: drops run state and memory. |
| `update_slot` | Takes the memory beside the slot: `update_slot(tree, &mut slot, &mut memory, ctx, entry)`. |
| Bevy `Behavior` | Holds memory beside the state. `BehaviorNode` pins `Memory` to a named associated type, as it pins `State` to `Data`. `restart` keeps memory; removing the component (`stop_behavior`) drops it. |

### Debugging

`inspect` receives every node's memory, active or not, so `describe()` and
traces can show a cooldown's last start on a branch that is not running.
Nodes report it as fields, as scope locals are reported today.

## Migration

For each custom node:

```rust
impl BtNode<Ctx> for MyNode {
    type State = ..;
    type Memory = ();                                  // add
    fn update(&self, state: &mut .., _: &mut (), ..)   // add the argument
    fn inspect(&self, state: Option<&..>, _: &(), ..)  // if overridden
}
```

Custom `BtControl` and `BtOrder` implementations add `type Memory = ();` and
the argument to their callbacks. About 75 `BtNode` implementations in the
repository change, nearly all mechanically.

## Alternatives

- **A slot count per node (`const MEMORY: usize`, defaultable), summed into one
  array in `BtState`.** Sizing an array by a generic constant needs
  `generic_const_exprs`, which is unstable.
- **A default `Memory = ()`.** Associated type defaults are unstable, and a
  blanket implementation of a separate memory trait conflicts with the nodes
  that override it without specialisation.
- **Memory inside `State`, kept by the parent.** A control keeps only one
  child's state; keeping all children's would make every tree's run state the
  sum of its branches, which is what the flat layout exists to avoid.
- **One framework store in the blackboard, keyed by node address.** No core
  change and one field instead of one per use, but the tree writes the
  blackboard, and Bevy's `gather` would have to leave that field alone.
- **An accessor per use** (`cooldown(span, |g| &mut g.dash_at, ..)`). Works
  today; the field-per-use is what this proposal removes.

## Plan

1. Core: the three traits, tuple children, `ControlNode`, leaves, `guard`,
   `BtState`, `update_slot`; tests that memory survives completion, `Evaluate`,
   preemption and a rejected candidate, that `reset` keeps it and `forget`
   drops it, and that a tree without memory has zero-sized memory.
2. Scope, catalog, orders and `inspect`/trace forwarding.
3. Bevy `Behavior`.
4. Then, separately: time (`BtClock`), `action_wait`, `timeout`,
   `reevaluate_every`, `cooldown` and `success_cooldown` on top of it.

## Open questions

- `forget()` as the name, versus `reset_memory()`.
- Whether `Memory` needs `Sync` in core, or only in Bevy's `BehaviorNode`, as
  `State` does today.
