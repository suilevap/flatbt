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

`bench` takes `AGENTS=`, `THREADS=`, `TREE=`, `SPLIT=1` (one tree under three
names, to measure what splitting costs) and `HANDROLLED=1` (the same trees
ticked by a system written by hand rather than generated from the context, in
`src/handrolled.rs`).

That last one answers whether `BehaviorContext` belongs in the library at all.
It does not have to: the hand-written tick is forty-five lines of ordinary Bevy
against thirty of declaration, plus a type alias for the query tuple that
`#[derive(QueryData)]` was providing. Back to back over 100 000 agents it is
about 8% faster on the serial path (2.00-2.22 ms against 2.31-2.36) and has no
parallel path at all, where the generated system runs at 1.19-1.26 -- so the
generator is about 1.7x ahead overall, on knowledge rather than code: per-batch
command queues, the write-back skip, and an order that cannot be got wrong.

It is its own workspace, so the repository's checks never build Bevy's renderer.

## What is in it

- `src/ai.rs` -- one `Fighter` context, three trees (`chaser`, `sniper`,
  `coward`) over it. The coward switches between the other two behaviours with
  `choose!`, and hides using a query its context never declared.
- `src/world.rs` -- the components, the one shared read-only resource every
  tree reads, `resolve_cover_requests` answering a question the trees cannot
  ask for themselves, and `apply_movement` / `fire_and_reload` carrying out what
  they decided.

## Trees decide; systems carry out

The blackboard has two halves that do not overlap. Everything above `intent` is
what the world said: `read` writes it, the tree only reads it. `intent` is what
the tree decided: only the tree writes it, and `write` carries it back out. No
field is both, and `FighterAccess` makes that the borrow checker's business
rather than a convention -- every component in it is `&` except `&mut Intent`.

```rust
fn shoot(bb: &mut Blackboard<Fighter>) -> NodeResult {
    bb.intent.shoot = true;      // not `bb.ammo -= 1`
    NodeResult::Success
}
```

A tree that subtracted the round itself would be deciding what a shot costs,
which belongs to the weapon. It can be wrong about whether it *should* shoot; it
cannot be wrong about how much ammunition a shot takes, because it never knew.
The same for movement: the tree names a destination and never learns how fast
this agent is or what a frame is worth.

It reads better in the tests, which is where it shows most: they assert on
decisions (`intent.move_to == Some(player)`) rather than on consequences
(`position.x < 100.0`), so a failure says which judgement was wrong instead of
which number moved.

Measured back to back at 100 000 agents, against the version where trees wrote
`position`, `health` and `ammo` directly:

| | AI tick, serial | AI tick, parallel | whole frame, serial | whole frame, parallel |
| --- | --- | --- | --- | --- |
| trees write components | 2.58-2.65 ms | 1.46-1.55 ms | 2.95-2.99 ms | 1.70-1.79 ms |
| trees write intents | 2.27-2.46 ms | 1.18-1.22 ms | 2.93-3.23 ms | 1.72-1.80 ms |

The tick gets about a tenth cheaper serially and a fifth in parallel, because
`write` now touches one small component instead of three. The frame does not pay
for it, because what moved out is two systems that are pure per-entity work and
spread across the pool as readily as the tick does -- `bin/bench` reports both
columns so that relocation cannot be mistaken for a saving.

## Numbers

Three trees over one context, mixed evenly, ticking every frame. Wall time
between `start_timing` and `stop_timing`, which bracket `BehaviorSystems`, on
a 4-core Xeon at 2.8 GHz:

| agents  | AI tick | with `.parallel()` | speedup | whole frame | frame, parallel |
| ------- | ------- | ------------------ | ------- | ----------- | --------------- |
| 10 000  | 0.32 ms | 0.30 ms            | 1.06x   | 0.58 ms     | 0.57 ms         |
| 50 000  | 1.27 ms | 0.75 ms            | 1.68x   | 1.80 ms     | 1.19 ms         |
| 100 000 | 2.46 ms | 1.23 ms            | 2.00x   | 3.18 ms     | 1.81 ms         |
| 200 000 | 4.39 ms | 2.37 ms            | 1.85x   | 5.34 ms     | 3.24 ms         |
| 400 000 | 8.57 ms | 4.32 ms            | 1.98x   | 10.0 ms     | 5.66 ms         |

About 21 ns per agent per tick serially. The frame columns are there because
work moved out of the tick has to show up somewhere, and without them a
relocation reads as a saving. Below ~5 000 agents the task pool costs more than
it saves and `.parallel()` is a loss, which is why it is opt-in rather than the
default.

One caution about every absolute number here: they were taken on a shared
container whose throughput drifts. The same unchanged binary measured 4.2 ms
over 100 000 agents one afternoon and 2.5 ms the next. Only figures taken back
to back in one session compare — which is what `bin/scaling` is for, and why
the claims elsewhere in this file name an A against a B rather than a
millisecond.

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
