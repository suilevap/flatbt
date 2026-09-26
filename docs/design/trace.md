# Trace: why the tree chose what it runs

Status: phases 1 and 2 implemented: `Entry`, node ids, the calls log and the
trace view. Per-node records (phase 3) and Bevy (phase 4) are proposals.

`describe()` shows *what* runs. A trace shows *why*: which nodes the last
update entered, how, what each returned and decided, including branches that
were tried and dropped, and a tree that failed outright.

## Format

A trace is one more view of the `inspect` walk, formatted only when asked:

```rust
state.set_trace(true); // no-op in release
let _ = update(&tree, &mut state, &mut ctx, EntryMode::Evaluate);
println!("{:#}", state.trace());
```

```text
select {next: RunChild(1)} → Running
  attack (scope) → Failure
    burst (compute) → Success
    seq → Failure
      has_ammo (check) → Failure    ← cause
  reload (leaf) → Running
```

- One line per node the last update entered: `label => name (kind)`, then
  `(resume)` or `(evaluate)` for a saved child (fresh candidates unmarked),
  then recorded values in braces, then `→ outcome`.
- Nodes the update did not enter are left out. Live fields of the running
  path -- scope locals, counters -- appear as in `describe()`.
- `← cause` marks where a failure started: a node that failed while none of
  its children did. A `select` that fails has one cause per branch.
- `{}` writes the running path on one line, with outcomes.

A utility selector shows its scores and the order it tried children in:

```text
select {order: by_score, score: [0.2, 0.9, 0.5], pick: [1, 2]} → Running
  needs.fatigue => leaf → Failure    ← cause
  0.5 => leaf → Running
```

A tree that ended with `Failure` has no state left, but its trace is complete:
the log does not depend on state surviving.

## Requirements

- **One update path.** The ordinary `update` records; no traced copy.
- **Passed, not global.** The log reaches nodes as an argument of `update`;
  no thread-local, no static.
- **Nothing in release.** Recording exists only with `debug_assertions` and
  `std`. Elsewhere the trace handle is zero-sized and its calls are empty
  `#[inline(always)]` functions, and no type grows. No Cargo feature: the
  build profile is the switch.
- **Nothing until a trace is on.** A dev build with no trace on checks one
  `Option` per node call, and holds no memory.
- **Typed records, text on demand.** Nodes log values of their own types.
  Text is made only when a trace is formatted, by each node's `inspect`.
- **Heap, but no allocation per frame.** The log is allocated when a trace is
  turned on and reused across updates.

## The log

One per traced agent, owned by its `BtState` (or Bevy `DebugBehavior`),
allocated by `set_trace(true)`. It has two parts.

**Calls**, written by the core for every node: one flat buffer of

```rust
struct Call {
    node: NodeId,     // preorder index in the definition
    entry: Entry,     // New, Resume, Evaluate
    outcome: Outcome, // Success, Failure, Running
}
```

in the order calls ended. Enough for the tree of calls, outcomes and
`← cause`, for any node, custom ones included.

**Records**, written by nodes, typed per node type. A node logs values of its
own types, and reads them back in its own `inspect`:

```rust
// in update: the closure runs only while a trace is on
entry.record(|| Picked { position, child });

// in inspect: this node's records from the last update, if any
for picked in inspector.records::<Picked>() {
    inspector.field("pick", &picked.child);
}
```

The log keeps one slot per node id that records anything: a `Box<dyn Any>`
holding that node's `Vec<R>`, created the first time the node records. Reading
and writing downcast by `TypeId`; no `unsafe`. A node recording two types
keeps one enum.

`inspector.records::<R>()` is a provided method over an object-safe accessor
the trace view implements; any other inspector returns nothing, so the same
`inspect` serves `describe()` and the trace.

**No allocation per frame.** Every buffer is cleared at the start of an update
and keeps its capacity; slots stay allocated. After the first updates that
reach each node, an update allocates only when it records more than any update
before it. A test counts allocations over repeated traced updates, as
`flatbt-bevy`'s allocation test does for ticks.

`set_trace` takes a limit on calls per update; past it recording stops and the
trace ends with `… (limit reached)`, so a `repeat` restarting many times
cannot grow the log without bound.

### What the catalog records

| Node | Record |
| --- | --- |
| every node | a `Call`, from its call site |
| `seq`, `select`, custom `BtControl` | each policy answer, `ControlOp` |
| `guard`, `if_else`, `repeat_while`, `reevaluate_when`, `action_while` | the condition, `bool` |
| `choose!` | the arm, a child index |
| `order_by` | each position's pick |
| `by_score`, `weighted` | each child's score or weight |
| `repeat`, `retry` | the count after each child |
| actions | started, completed |
| diagnostics | the message, on the node that raised it |

Records hold values, not text; text is made by `inspect` when the trace is
formatted. A record must be `'static`; to be shown, its node's `inspect` needs
it `Debug`. `by_score`'s generic score type therefore gains a `Debug` bound,
which every numeric type meets. Diagnostics are the one exception: a message
is `Display`, not a value, so it is written into a text buffer of the log when
raised -- only while a trace is on.

Custom nodes log the same way: `entry.record` in `update`,
`inspector.records` in an overridden `inspect`. Without that, they still get
their `Call`s.

## Passing the log: `Entry`

The log belongs to the agent and reaches nodes as an argument, the way the
entry mode already does. `update`'s `mode: EntryMode` becomes an `Entry`.

Each kind of data a node sees then has one owner and one lifetime:

| Data | Owner | Lives |
| --- | --- | --- |
| Context (`ctx`), the blackboard | the application | across invocations |
| Invocation state (`BtState`) | the agent | while the tree runs |
| Scope locals (`params`) | a `scope` in that state | while the scope runs |
| `Entry` | the update | one call |

`Entry` is the per-update part: how this node was entered, and in dev builds
where this update is being recorded.

```rust
fn update(&self, state: &mut Self::State, ctx: &mut C, params: P, entry: Entry<'_>) -> NodeResult<A>;

#[derive(Clone, Copy)]
pub struct Entry<'t> {
    mode: EntryMode,
    trace: TraceHandle<'t>, // dev builds: the agent's log, if on, and this node's id
}                           // release: zero-sized
```

- **It is the mode, plus the trace.** `entry.mode()` replaces `mode`. In
  release `TraceHandle` is zero-sized, so `Entry` is the same byte as
  `EntryMode` and `update` compiles to the same code.
- **`Copy`, like `EntryMode`.** A control passes it to several children in one
  update, as it passes the mode now. The handle holds a shared
  `&RefCell<TraceLog>`, so copies can all record; `RefCell` is `core`.
- **Children get their own.** A parent calls a child with
  `entry.child(offset)`, or `entry.candidate(offset)` for a fresh candidate,
  which also switches the mode to `Evaluate`. Both set the child's node id.
- **Nodes record through it:** `entry.record(|| Picked { position, child })`.
  The closure runs only while a trace is on; in release the call is empty.
- **Drivers pass the log.** `update` takes it from the `BtState`;
  `update_slot` gains a variant taking `Option<&TraceLog>`, which Bevy's tick
  uses for agents carrying a traced `DebugBehavior`.

No thread-local, no global, and one signature in every build. Tracing works
wherever the log can be allocated: `std`, or later `alloc`.

The cost is a breaking change: every node's `update` takes `entry: Entry<'_>`
instead of `mode: EntryMode`. The migration is mechanical -- rename the
argument, read `entry.mode()`, pass `entry` on -- and it gives custom nodes the
same way to record as the catalog's.

Policies and orders do not run `update`; their records go through the node
that calls them. `ControlNode` records each policy answer itself. `BtOrder`
gets a defaulted `record(&self, ctx, index, recorder)` that `Ordered` calls
while a trace is on, for scores and weights.

## Node ids

A record names its node by a preorder index into the definition, which is the
same for every agent and every update. So the log needs no state to point at:
dropped candidates and a finished tree are still addressable.

- `BtNode` gets `const NODES: usize = 1`: the size of the subtree. Associated
  consts have defaults on stable, so custom nodes need nothing.
- Composites sum their children: the tuple impls add each child's `NODES`, and
  single-child nodes add one.
- The offset a parent passes to `entry.child(offset)` is a constant: one, plus
  the `NODES` of the children before it.

A custom composing node keeps `NODES = 1` by default: its descendants then
share its id and their records merge into its line. It can declare its
subtree size and pass offsets to give each child its own id.

## Merging into `inspect`

Formatting walks the definition with `inspect`, as `describe().with_inactive()`
does, numbering nodes in the same preorder. Each node's records are attached
to its line; nodes without records are hidden. State supplies the live fields
of the running path, the log everything else. Plumbing that inspection hides
-- `bind`, `named`, `scope!`'s initializer sequence -- keeps its id so the
numbering matches; `scope!`'s hidden sequence becomes a flag in `NodeInfo`
rather than a swallowed `enter`.

## Bevy

`DebugBehavior::traced()` owns a log for its agent. In dev builds the tick
passes it to `update_slot` for marked agents only; `Behavior` does not grow. The component
keeps the last trace's records and formats on request, like `path()`.

## Costs

| Build | Cost |
| --- | --- |
| Release, or no `std` | None: `Entry` is the mode alone. |
| Dev, no trace on | `Entry` carries an empty handle: an `Option` check per node call. |
| Dev, trace on | A `RefCell` borrow and a push per call and record; the log on the heap, reused across updates, no allocation per frame once warm. |

The call-site hook sits inside the tuple `run_from` that PR #24 tuned. Release
equivalence is checked by comparing release assembly of a deep tree's update
before and after, and the suite runs in both profiles.

## Alternatives

- **Inline marks in state** (the previous draft). Two fixed bytes cannot hold
  node-specific values such as scores, and marks die with the state holding
  them: a rejected branch's details, and the whole tree when it fails.
- **Records keyed by state addresses.** State moves when a candidate is
  selected and when a `BtState` moves; preorder ids do not.
- **Node-specific records stored inline** (a score array beside `by_score`'s
  state). Typed exactly, but lost with the state holding them.
- **One universal value enum** (the previous draft). Nodes would squeeze their
  values into a fixed vocabulary, and anything outside it would be formatted
  when recorded. Per-type records keep values as they are.
- **A separate `update_traced` path.** A second update path to keep in step.
- **A thread-local lent the log for each update** (the previous draft). No
  signature change, but a global channel in disguise, and none without `std`.
- **A trait on the context** (`C: TraceContext`). Every context would need an
  impl -- impossible for a foreign type such as `u32` under the orphan rule --
  and `focus` hands its subtree a different context, which would need one too.
- **A trace handle in the parameters.** Parameters are typed per node and
  replaced below each `scope`, so the handle would not reach every node.
- **A runtime flag in release**, or **dry-run explain** over `&C`: cost in
  shipping builds, or what the tree would choose now rather than what it did.

## Phases

1. `Entry` replacing `EntryMode` in `update`, migrated across the crate, the
   examples and the Bevy crate; release assembly check. Done: release
   assembly of the `acts`, `choose` and `resume` examples is instruction for
   instruction the same as before.
2. `NODES`, ids through `entry.child`, the log with `Call`s, `set_trace`, the
   trace view with `← cause`; allocation test.
3. `entry.record` and `records::<R>()`; policy answers, conditions,
   `choose!`, orders and scores, counters, actions, diagnostics.
4. Bevy: `DebugBehavior::traced()`.

## Open questions

- Keep only the last update's log, or a ring of the last few updates to show
  when a decision changed.
- Whether `← cause` should also follow `Resume` failures, where the policy
  never consulted the alternatives.
