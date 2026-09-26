# Comparison with other Rust behavior tree crates

A standalone crate, outside the workspace, that runs the same trees on FlatBT
and four other crates and measures time, instructions, allocation and memory.

```sh
cd benchmarks
cargo run --release                      # full run, ~4 minutes
cargo run --release -- --quick           # shorter samples
cargo run --release -- --scenario guard --lib bhv
./instructions.sh                        # cachegrind; needs valgrind
csharp/compare.sh                        # against bt-tree (C#); needs .NET 10
```

The run exits non-zero if any library's results differ from FlatBT's.

## Libraries

| Crate | Version | Model |
| --- | --- | --- |
| flatbt | this repository | Static dispatch, one shared tree, per-agent state inline |
| [bonsai-bt](https://crates.io/crates/bonsai-bt) | 0.14.0 | `Behavior` enum tree; `BT` owns a clone and rebuilds child state from it on each transition |
| [behavior-tree](https://crates.io/crates/behavior-tree) | 0.1.0 | `Rc<RefCell<Node>>` tree, state in the nodes |
| [bhv](https://crates.io/crates/bhv) | 0.4.0 | `Box<dyn Bhv>` tree, state in the nodes |
| [behavior-tree-lite](https://crates.io/crates/behavior-tree-lite) | 0.3.2 | `Box<dyn BehaviorNode>` containers after BehaviorTree.CPP; agent data through a `dyn Any` callback |

Left out: Bevy plugins (`bevy_behave`, `bevior_tree`), whose cost is the ECS
around them; `behaviortree-rs`, an async BehaviorTree.CPP port; `forester-rs`, a
DSL interpreter.

## Method

Every library runs the same trees over the same blackboard and the same
leaf function, [`Op::run`](src/common.rs). A tick advances a small
deterministic world, then ticks the tree once. After 100k ticks each library's
blackboard and Success/Failure/Running counts must equal FlatBT's; all do.
bonsai-bt and behavior-tree have a closed set of node kinds, so they run the
first four trees only.

| Scenario | Tree | Shape of the work |
| --- | --- | --- |
| `select8` | `select` of 8 × `seq(is_mode(i), act(i))` | Completes every tick; condition scan with an unpredictable winner |
| `patrol` | `seq(move_to(8), act, move_to(-8), act)` | Running 97% of ticks; resumes the active child |
| `guard` | `select(seq(low_hp, move_to(0), heal), seq(enemy, attack), patrol)` | Three levels; mostly resuming, some rescans |
| `soldier` | A game NPC, below: 68 nodes, 43 leaves, depth 7 | A realistic mix of rescans, resumes and aborts |
| `villager` | Needs by utility, below: 51 nodes, 28 leaves, depth 7 | FlatBT's node catalog: orders, repeats, decorators |

`soldier` ([source](src/soldier.rs)) is ordered by priority. Its world spawns
enemies that shoot back and close in, makes noises, and raises hunger and
fatigue every tick:

```text
select
├─ seq  dead? → respawn (20 ticks)
├─ seq  low health? → select
│        ├─ seq  has medkit? → use medkit (3)
│        ├─ seq  enemy visible? → go to cover → regenerate (6)
│        └─ seq  go to base → regenerate (6)
├─ seq  enemy visible? → select
│        ├─ seq  magazine empty? → select
│        │        ├─ seq  has reserve? → reload (4)
│        │        ├─ seq  enemy close? → melee (3, fails if enemy gone)
│        │        └─ seq  go to ammo → take ammo
│        ├─ seq  enemy in range? → select
│        │        ├─ seq  has grenade? → enemy grouped? → throw
│        │        └─ seq  aim (2, fails if enemy gone) → fire
│        └─ close in (fails if enemy gone)
├─ seq  heard noise? → go to noise* → look around (3)* → clear noise
├─ seq  hungry? → select
│        ├─ seq  has food? → eat (5)*
│        └─ seq  go to kitchen* → take food
├─ seq  tired? → go to bed* → sleep (12)*
└─ seq  patrol: go to 12* → look (3)* → go to -12* → look (3)* → go to 0* → lap done
                                                  * fails when an enemy appears
```

Over 100k ticks the soldier fires 3,131 shots, reloads 345 times, throws 98
grenades, uses 98 medkits, eats 214 times, sleeps 116 times, completes 585
patrol laps and respawns 48 times. The run prints the full mix.


`villager` ([source](src/villager.rs)) exercises FlatBT's node catalog. Needs
grow every tick, threats come and go, and every random choice draws from one
seeded generator on the blackboard:

```text
select
├─ seq  threat? → if_else(healthy?)
│        ├─ then: seq  repeat(3, swing (2, fails if threat gone))
│        │             → force_success(seq  loot? → take loot)
│        └─ else: seq  invert(cornered?) → go to safety → hide (4)
└─ utility by need score (per_child!)
         ├─ hunger:  seq  retry(3, forage: 2 in 3 by draw) → go to kitchen*
         │                → repeat(2, chew (3)*) → eat
         ├─ fatigue: seq  go to bed* → repeat_while(tired?, rest (2)*)
         ├─ boredom: weighted_select by friendship (per_child!)
         │           ├─ seq  friend 0 home? → go to friend 0* → chat (3)*
         │           ├─ … friend 1
         │           └─ … friend 2
         └─ duty:    shuffle_seq
                     ├─ seq  go to woodpile* → chop (2)*
                     ├─ seq  go to well* → carry water (2)*
                     └─ seq  go to hall* → sweep (2)*
                                          * fails when a threat appears
```

Over 100k ticks the villager swings 4,080 times, loots 659 times, hides 63
times, eats 765 meals, rests 952 times, chats about 260 times with each friend
and does about 330 of each chore, from 7,231 random draws.

No other crate here ships utility, weighted or shuffled selection, so bhv and
behavior-tree-lite get them as custom nodes written against their public node
traits, with FlatBT's selection algorithms ported exactly (`Order::next` in
[`src/common.rs`](src/common.rs)); `repeat`, `retry`, `if_else` and
`repeat_while` likewise. Their own inverter and force-success nodes match
FlatBT's and are used as they are.

| FlatBT | bonsai-bt | behavior-tree | bhv | behavior-tree-lite | bt-tree (C#) |
|---|---|---|---|---|---|
| `utility`, `by_score` | — | — | — | — | — |
| `random_select`, `shuffle_seq`, `weighted_select` | — | — | — | — | — |
| `if_else` | `If` | `Cond` | — | `if` | — |
| `invert`, `force_success`, `force_failure` | `Invert`, `AlwaysSucceed` | — | `Inv`, `Pass`, `Fail` | `Inverter`, `ForceSuccess`, `ForceFailure` | `Optional` |
| `repeat`, `retry` | — | — | `Repeat`, `RepeatUntilPass` (different counting) | `Repeat`, `Retry` (count from a blackboard port) | `RepeatUntilSuccess` |
| `repeat_while`, `guard` | `While` | `While` | `RunIf`, `RepeatUntil` | — | `While` |

In Rust, utility and random selection exist only in Bevy plugins:
`bevior_tree`'s score- and random-ordered sequences and selectors, `gamai`'s
`UtilitySelector`, `bevy_thinker`. Their cost is the ECS around them, so they
are not measured here.

Choices that keep the trees equivalent:

- Controls keep their place (non-reactive). FlatBT ticks with `EntryMode::Resume`.
  Interruption is written into the leaves: a long action fails when the world
  invalidates it, the failure unwinds to the root, and the next tick chooses
  from the top. Every crate can express that; not every crate has a guard.
- Multi-tick leaves keep progress on the blackboard, not in the node, because
  `bonsai-bt` actions cannot hold state. FlatBT's inline action state
  (`BtAction`) is therefore not exercised.
- Under `Resume` an ordered control never reorders a running child, so the
  ports only need FlatBT's orders for the no-running-child case.
- Each crate restarts a finished tree its own way: FlatBT clears the slot;
  bonsai-bt calls `reset_bt`; the others restart on the next tick.
- Agents share one tree where the crate allows it. Only FlatBT does: bonsai-bt
  clones the `Behavior` into each `BT`, and the others keep state in the nodes,
  so every agent builds its own tree.

Metrics:

| Column | Meaning |
| --- | --- |
| ns/tick, 1 agent | Median of 7 samples of 2M ticks; tree and state stay in L1 |
| ns/tick, 10k agents | Median of 7 samples of 200 frames × 10,000 agents, clocks staggered |
| allocs/tick, bytes/tick | Heap allocations during 100k steady-state ticks, via a counting global allocator |
| bytes/agent | `size_of` the per-agent runtime + heap it keeps after construction |
| build | Allocations and time to create one agent, averaged over 10,000 |
| peak heap growth | Highest heap above the built population while 10,000 agents tick |
| shared tree | `size_of` + heap of the one tree all agents run |
| instructions, mispredicts | cachegrind, per tick: (2N-tick run − N-tick run) / N |

The blackboard itself is excluded from bytes/agent. The world step
costs under 2 ns/tick and is included in every time.

## Results

FlatBT at `main` 0a6d668 (with #17 and #18), Rust 1.98.1, 4-vCPU Intel Xeon @
2.10 GHz cloud VM, `lto = "fat"`, `codegen-units = 1`. Wall-clock varies by up
to about 15% between runs on this shared machine; the instruction counts do
not vary. Ratios are against FlatBT.

### select8

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 34.8 | 42.3 | 0 | 0 | 2 + 0 | 0 | 126 | 0 | 384 |
| bonsai-bt | 1133.8 (32.6×) | 1711.9 (40.5×) | 19.00 | 1628.0 | 112 + 2112 | 21 | 2466 | 1152 | — |
| behavior-tree | 304.8 (8.8×) | 1577.7 (37.3×) | 5.50 | 44.0 | 104 + 6016 | 81 | 6025 | 16 | — |
| bhv | 47.0 (1.4×) | 121.1 (2.9×) | 0 | 0 | 16 + 1056 | 34 | 1433 | 0 | — |
| behavior-tree-lite | 190.5 (5.5×) | 1306.7 (30.9×) | 0 | 0 | 248 + 5328 | 44 | 2424 | 0 | — |

### patrol

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 6.8 | 8.1 | 0 | 0 | 1 + 0 | 0 | 60 | 0 | 96 |
| bonsai-bt | 20.7 (3.0×) | 38.6 (4.8×) | 0.06 | 6.5 | 112 + 376 | 3 | 272 | 216 | — |
| behavior-tree | 47.0 (6.9×) | 84.8 (10.5×) | 1.00 | 8.0 | 104 + 1040 | 17 | 726 | 8 | — |
| bhv | 7.6 (1.1×) | 23.1 (2.9×) | 0 | 0 | 16 + 192 | 6 | 283 | 0 | — |
| behavior-tree-lite | 33.2 (4.9×) | 55.1 (6.8×) | 0 | 0 | 248 + 592 | 7 | 386 | 0 | — |

### guard

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 9.5 | 16.9 | 0 | 0 | 2 + 0 | 0 | 51 | 0 | 216 |
| bonsai-bt | 55.4 (5.9×) | 185.5 (11.0×) | 0.49 | 47.8 | 112 + 1192 | 11 | 665 | 61632 | — |
| behavior-tree | 84.9 (9.0×) | 482.5 (28.6×) | 2.07 | 16.5 | 104 + 3036 | 43 | 1798 | 16 | — |
| bhv | 10.8 (1.1×) | 67.9 (4.0×) | 0 | 0 | 16 + 536 | 17 | 857 | 0 | — |
| behavior-tree-lite | 51.3 (5.4×) | 264.8 (15.7×) | 0 | 0 | 248 + 2200 | 21 | 1089 | 0 | — |

### soldier

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 21.6 | 38.1 | 0 | 0 | 3 + 0 | 0 | 52 | 0 | 1032 |
| bonsai-bt | 349.5 (16.2×) | 1659.5 (43.6×) | 5.15 | 483.0 | 112 + 5552 | 53 | 3152 | 3840800 | — |
| behavior-tree | 162.5 (7.5×) | 1558.9 (40.9×) | 3.25 | 26.0 | 104 + 16748 | 221 | 15184 | 48 | — |
| bhv | 26.2 (1.2×) | 213.4 (5.6×) | 0 | 0 | 16 + 2904 | 93 | 3229 | 0 | — |
| behavior-tree-lite | 78.8 (3.6×) | 902.4 (23.7×) | 0 | 0 | 248 + 14392 | 120 | 6433 | 0 | — |

### villager

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 23.5 | 44.0 | 0 | 0 | 40 + 0 | 0 | 60 | 0 | 720 |
| bhv | 31.2 (1.3×) | 239.0 (5.4×) | 0 | 0 | 16 + 2240 | 67 | 2439 | 0 | — |
| behavior-tree-lite | 88.5 (3.8×) | 1187.4 (27.0×) | 0 | 0 | 248 + 12200 | 125 | 7088 | 0 | — |

### Instructions and branch mispredictions per tick (one agent)

| library | select8 | patrol | guard | soldier | villager |
|---|--:|--:|--:|--:|--:|
| flatbt | 452 / 7.57 | 107 / 0.55 | 134 / 0.87 | 215 / 2.47 | 354 / 1.63 |
| bonsai-bt | 8307 / 82.63 | 284 / 2.77 | 583 / 5.87 | 2595 / 31.68 | — |
| behavior-tree | 3234 / 38.30 | 453 / 2.83 | 864 / 4.96 | 1565 / 14.90 | — |
| bhv | 531 / 4.43 | 109 / 0.42 | 155 / 0.65 | 262 / 3.04 | 338 / 2.76 |
| behavior-tree-lite | 1982 / 13.38 | 354 / 0.53 | 545 / 2.88 | 845 / 5.67 | 961 / 5.22 |

## Reading the results

- **Memory.** A FlatBT agent is its state and never touches the heap: 1–3
  bytes for the first four trees, 40 for `villager`, and one tree of 96 bytes
  to 1 KB serves every agent. The others need 208 bytes (`bhv`, `patrol`) to
  16.9 KB (`behavior-tree`, `soldier`) per agent, built in 6–221 allocations.
- **Allocation while ticking.** FlatBT, `bhv` and `behavior-tree-lite` allocate
  nothing. bonsai-bt rebuilds child state from a clone of the `Behavior` on
  every transition and on `reset_bt`: 5 allocations per `soldier` tick, 19
  per `select8` tick, and 3.8 MB of heap growth while 10k soldiers tick.
  behavior-tree builds a debug string in every sequence tick.
- **Many agents.** FlatBT is 2.9–5.6× faster than the next crate, `bhv`, and
  4.8–44× faster than the rest. The gap widens with tree size, because a
  FlatBT agent's working set is a few bytes while the per-agent boxed trees
  spill out of cache.
- **One hot agent.** With everything in L1 and the `dyn` calls predicted,
  `bhv` comes closest: 1.1–1.4× FlatBT's time, with 2–22% more instructions,
  except on `villager`, where it executes 5% fewer (338 against 354).
- **The node catalog.** `villager` costs FlatBT about as much per tick as
  `soldier` and allocates nothing. Its 40 bytes per agent come from the
  catalog's counters: `repeat` and `retry` keep a `usize`, and an ordered
  control a `u64` of used children plus its position, 16 bytes each with
  alignment where a plain `seq` or `select` needs one.

## No regressions

The same benchmark built against earlier FlatBT revisions (`CARGO_FLAGS=--no-default-features ./instructions.sh`
drops the catalog-only tree), instructions per tick for one agent:

| FlatBT | select8 | patrol | guard | soldier |
|---|--:|--:|--:|--:|
| b1a3130, before #17 and #18 | 592 | 107 | 168 | 295 |
| 8d5a7ff, #17 and #18 merged | 452 | 107 | 134 | 215 |
| 0a6d668, with the node catalog | 452 | 107 | 134 | 215 |

At every revision FlatBT allocates nothing per tick, nothing to build an
agent, and nothing while 10k agents tick, and keeps 1–3 bytes per agent on
these trees.

## Against bt-tree (C#)

[bt-tree](https://github.com/suilevap/bt-tree) is the C# library FlatBT
descends from: one shared tree, a per-agent `Context` holding the running path
as `NodeContext` objects. Different runtime, so this measures how far the Rust
design goes, not Rust against C#.

```sh
csharp/compare.sh            # needs a .NET 10 SDK; fetches bt-tree at 90f5002
csharp/compare.sh --quick
```

[`csharp/Program.cs`](csharp/Program.cs) ports the world, the leaves and all
four trees line for line and compiles bt-tree's `BTLib` sources unchanged into
the project, as its README says to use it. Conditions are `bt.Condition`,
instant actions `bt.Action`, and multi-tick leaves subclass `ActionNode`.

bt-tree's `Context.Update()` rethinks the tree every update: selectors rescan
from their first child, sequences resume their running child. That is FlatBT's
`EntryMode::Evaluate`, so this comparison runs FlatBT with `Evaluate`
(`flatbt-compare evaluate`), not the `Resume` used above. The script checks
that both produce the same checksum on every scenario; they do.
`bt-tree-pooled` passes bt-tree's own `PoolNodeContext` instead of allocating
a `NodeContext` per visited node.

.NET 10.0.12 (JIT, tiered PGO, workstation GC, 2M ticks of warm-up per run),
FlatBT and the machine as above. `villager` is not ported: bt-tree has no
ordered controls, and matching FlatBT's `Evaluate` revalidation in custom ones
is a port of its own. The .NET population timings varied by up to 50% between
runs, with garbage collection; the ratios below are from one run. Bytes/agent
for bt-tree is measured after ticking, when running paths hold their node
contexts.

| scenario | library | ns/tick, 1 agent | ns/tick, 10k agents | bytes allocated/tick | bytes/agent | ns to build an agent |
|---|---|--:|--:|--:|--:|--:|
| select8 | flatbt | 28.4 | 30.9 | 0 | 2 | 142 |
|  | bt-tree | 237.2 (8.4×) | 253.2 (8.2×) | 440 | 488 | 103 |
|  | bt-tree-pooled | 201.3 (7.1×) | 234.0 (7.6×) | 0 | 487 | 108 |
| patrol | flatbt | 5.4 | 8.3 | 0 | 1 | 128 |
|  | bt-tree | 78.7 (14.6×) | 85.3 (10.3×) | 83.6 | 568 | 105 |
|  | bt-tree-pooled | 71.5 (13.2×) | 79.3 (9.6×) | 0 | 593 | 86 |
| guard | flatbt | 15.9 | 24.7 | 0 | 2 | 54 |
|  | bt-tree | 170.5 (10.7×) | 198.7 (8.0×) | 248 | 597 | 95 |
|  | bt-tree-pooled | 168.1 (10.6×) | 227.9 (9.2×) | 0 | 606 | 90 |
| soldier | flatbt | 57.4 | 72.0 | 0 | 3 | 69 |
|  | bt-tree | 350.9 (6.1×) | 586.2 (8.1×) | 540.7 | 611 | 85 |
|  | bt-tree-pooled | 351.4 (6.1×) | 460.2 (6.4×) | 0 | 597 | 99 |

On `soldier`, FlatBT runs about 6× faster than bt-tree for one agent and 6–8×
for 10k, in 3 bytes per agent instead of about 600. Both share one tree, so
the difference is the per-agent path: bt-tree pushes a `NodeContext` object
per visited node, copies it from the previous path, and dispatches every node
through a virtual call and a delegate; FlatBT keeps the path as nested enum
tags and resolves it at compile time.

bt-tree's `Update(time, forceUpdate: false)` ticks only the running action
between rethinks. It changes the semantics, so it is not compared here.

Not measured: compile time, binary size, reactive (`Evaluate`) ticking,
parallel ticking.
