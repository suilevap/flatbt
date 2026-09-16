# arena

A load test for the FlatBT Bevy integration, shaped like a game: a few hundred
thousand enemies with three different minds, sharing one blackboard, in one
arena.

It exists because the integration was designed without a consumer. Everything
here is a coloured rectangle; the only thing it tries to look good at is the
frame budget.

```sh
cargo run --release --bin arena          # windowed, 20k enemies, [ and ] change that
cargo run --release --bin arena -- 100000
cargo run --release --bin bench          # headless, serial vs parallel
cargo run --release --bin scaling        # a control: what this machine's task pool can do
cargo test --release                     # the trees, with no Bevy app
```

It is its own workspace, so the repository's checks never build Bevy's renderer.

## What is in it

- `src/ai.rs` -- one `Fighter` context, three trees (`chaser`, `sniper`,
  `coward`) over it. The coward switches between the other two behaviours with
  `choose!`, and hides using a query its context never declared.
- `src/world.rs` -- the components, the one shared read-only resource every
  tree reads, and `resolve_cover_requests`: an ordinary Bevy system that
  answers a question the trees cannot ask for themselves.

## Numbers

Three trees over one context, mixed evenly, ticking every frame. Wall time
between `start_timing` and `stop_timing`, which bracket `BehaviorSystems`, on
a 4-core Xeon at 2.8 GHz:

| agents  | serial  | `.parallel()` | speedup |
| ------- | ------- | ------------- | ------- |
| 10 000  | 0.52 ms | 0.49 ms       | 1.07x   |
| 50 000  | 2.16 ms | 1.12 ms       | 1.93x   |
| 100 000 | 4.12 ms | 1.88 ms       | 2.20x   |
| 200 000 | 8.41 ms | 3.65 ms       | 2.30x   |
| 400 000 | 16.6 ms | 7.03 ms       | 2.37x   |

About 41 ns per agent per tick serially, of which roughly 1 ns is the
revalidation guard deciding whether this agent reconsiders at all. Below
~5 000 agents the task pool costs more than it saves and `.parallel()` is a
loss, which is why it is opt-in rather than the default.

## What building it changed

**The blackboard is a snapshot, not a pile of borrows.** `BehaviorContext`
gathers a plain struct before the tick and writes it back after. Nothing in a
tree, node or action signature carries a lifetime any more, node parameters work
again, and `tests/trees.rs` runs these very trees with no Bevy app at all. It
measured the same as the borrowed version -- but only after the per-agent
`CommandQueue` was made lazy: owning an empty one per agent per tick cost a
fifth of the whole tick, because dropping a `CommandQueue` walks its buffer
whether or not anything is in it.

**A tree that fails on resume re-enters from the root in the same tick.** A
resumed update never consults the branches above the one it resumed, so a
failure reached that way says nothing about what the tree would choose now.
`Behavior::tick` retries with `Evaluate`; core keeps `Resume` an honest resume.
It only saves a tick -- and only when the whole tree fails, which a `Running`
fallback below the failure prevents. That case needs `entry_mode`.

**`ask` is now a node the integration ships, and it feeds `scope!` locals.**
Asking the world something the context never declared was 25 lines of
hand-written `BtAction` per question. It is one line, and bound to an output
slot it hands the answer to the nodes after it as a plain value:

```rust
scope! {
    let spot: Vec2;
    sequence {
        ask(WantsCover, |bb: &Blackboard<Fighter>| bb.cover).with(out spot);
        WalkTo.with(spot);
    }
}
```

That is where the ECS and the scope meet. An ordinary system answers by writing
an ordinary component; `read` brings it into the snapshot; `ask` moves it into
the local. `WalkTo` takes a `Vec2`, not an `Option<Vec2>`, so it cannot run
without one — and the local belongs to the invocation, so leaving the branch
and coming back asks again instead of walking to a spot chosen for an older
situation.

**`evaluate_every` no longer divides 128-bit integers.** It ran two of them per
agent per tick, which on a real tree cost more than the tree: 57 ns per agent
against 36 ns now. It works in `u64` and takes one remainder instead.

**Commands are handed out per batch, not per agent.** The parallel tick used
`ParallelCommands::command_scope` around each agent. That takes a thread-local
borrow per call, and at the cost of a real tree it ate most of the speedup --
1.6x instead of 2.3x at 100 000 agents. It now builds one `CommandQueue` per
`par_iter` batch and appends them when the pass is over. `bin/scaling` is the
control that measures the difference in isolation.

## What building it found

**`Running` is a promise, and a leaf cannot keep it.** `take_cover` asked for
cover with a leaf that returned `Running` while it waited. Under `Resume` a
control node re-runs its active child directly, so the tree never left that
leaf: it re-inserted its request every frame, and the archetype churn cost
220 ns per agent per tick -- four times the whole tree -- while scaling to
exactly 1.0x however many threads it was given. The symptom read as
"parallelism does not work here", not as "this tree is wrong".

`Failure` would have been enough on its own. A control node whose resumed
child fails moves on to the next sibling, and an invocation that reaches a
terminal result is dropped, so the next tick starts fresh from the root as
`Evaluate`. Only `Running` sticks. A node that returns it is promising to make
progress and eventually stop, and a stateless leaf has nothing to keep that
promise with -- which is why asking is `ask`, an action whose `start` runs once
per invocation.

**Revalidation is for abandoning work, not for reacting.** With the asking
fixed, the coward switches between fighting and hiding without any entry mode
at all: each invocation ends, and the next one re-picks. `entry_mode` earns its
place only for the branch that does not end -- walking to cover takes a hundred
frames, and a coward healed halfway there should turn around. That costs about
7 ns per agent per tick, and `evaluate_every` spreads it so the population does
not all reconsider on one frame.
