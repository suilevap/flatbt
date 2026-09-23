# Comparison with other Rust behavior tree crates

A standalone crate, outside the workspace, that runs the same trees on FlatBT
and four other crates and measures time, instructions, allocation and memory.

```sh
cd benchmarks
cargo run --release                      # full run, ~3 minutes
cargo run --release -- --quick           # shorter samples
cargo run --release -- --scenario guard --lib bhv
./instructions.sh                        # cachegrind; needs valgrind
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

Every library runs the same four trees over the same blackboard and the same
leaf function, [`Op::run`](src/common.rs). A tick advances a small
deterministic world, then ticks the tree once. After 100k ticks each library's
blackboard and Success/Failure/Running counts must equal FlatBT's; all do.

| Scenario | Tree | Shape of the work |
| --- | --- | --- |
| `select8` | `select` of 8 × `seq(is_mode(i), act(i))` | Completes every tick; condition scan with an unpredictable winner |
| `patrol` | `seq(move_to(8), act, move_to(-8), act)` | Running 97% of ticks; resumes the active child |
| `guard` | `select(seq(low_hp, move_to(0), heal), seq(enemy, attack), patrol)` | Three levels; mostly resuming, some rescans |
| `soldier` | A game NPC, below: 68 nodes, 43 leaves, depth 7 | A realistic mix of rescans, resumes and aborts |

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

Choices that keep the trees equivalent:

- Controls keep their place (non-reactive). FlatBT ticks with `EntryMode::Resume`.
  Interruption is written into the leaves: a long action fails when the world
  invalidates it, the failure unwinds to the root, and the next tick chooses
  from the top. Every crate can express that; not every crate has a guard.
- Multi-tick leaves keep progress on the blackboard, not in the node, because
  `bonsai-bt` actions cannot hold state. FlatBT's inline action state
  (`BtAction`) is therefore not exercised.
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

Rust 1.94.1, 4-vCPU Intel Xeon @ 2.10 GHz cloud VM, `lto = "fat"`,
`codegen-units = 1`. Wall-clock varied by up to about 15% between two full
runs on this shared machine; the instruction counts do not vary. Ratios are
against FlatBT.

### select8

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 47.7 | 48.1 | 0 | 0 | 2 + 0 | 0 | 61 | 0 | 384 |
| bonsai-bt | 1132.8 (23.7×) | 1471.0 (30.6×) | 19.00 | 1628.0 | 112 + 2112 | 21 | 2308 | 1152 | — |
| behavior-tree | 303.7 (6.4×) | 1128.2 (23.5×) | 5.50 | 44.0 | 104 + 6016 | 81 | 5091 | 16 | — |
| bhv | 47.2 (1.0×) | 115.9 (2.4×) | 0 | 0 | 16 + 1056 | 34 | 1094 | 0 | — |
| behavior-tree-lite | 189.2 (4.0×) | 742.8 (15.5×) | 0 | 0 | 248 + 5328 | 35 | 1999 | 0 | — |

### patrol

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 5.2 | 7.1 | 0 | 0 | 1 + 0 | 0 | 30 | 0 | 96 |
| bonsai-bt | 16.3 (3.2×) | 31.5 (4.4×) | 0.06 | 6.5 | 112 + 376 | 3 | 197 | 216 | — |
| behavior-tree | 44.1 (8.5×) | 80.6 (11.4×) | 1.00 | 8.0 | 104 + 1040 | 17 | 712 | 8 | — |
| bhv | 8.4 (1.6×) | 18.9 (2.7×) | 0 | 0 | 16 + 192 | 6 | 257 | 0 | — |
| behavior-tree-lite | 26.9 (5.2×) | 55.0 (7.8×) | 0 | 0 | 248 + 592 | 6 | 312 | 0 | — |

### guard

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 10.1 | 16.3 | 0 | 0 | 2 + 0 | 0 | 30 | 0 | 216 |
| bonsai-bt | 56.7 (5.6×) | 169.5 (10.4×) | 0.49 | 47.8 | 112 + 1192 | 11 | 652 | 61632 | — |
| behavior-tree | 89.3 (8.8×) | 359.9 (22.0×) | 2.07 | 16.5 | 104 + 3036 | 43 | 1931 | 16 | — |
| bhv | 9.1 (0.9×) | 62.4 (3.8×) | 0 | 0 | 16 + 536 | 17 | 594 | 0 | — |
| behavior-tree-lite | 53.6 (5.3×) | 233.1 (14.3×) | 0 | 0 | 248 + 2200 | 17 | 940 | 0 | — |

### soldier

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 24.9 | 39.5 | 0 | 0 | 3 + 0 | 0 | 30 | 0 | 1032 |
| bonsai-bt | 354.7 (14.2×) | 1239.9 (31.4×) | 5.15 | 483.0 | 112 + 5552 | 53 | 3968 | 3840800 | — |
| behavior-tree | 162.3 (6.5×) | 1017.3 (25.8×) | 3.25 | 26.0 | 104 + 16748 | 221 | 12358 | 48 | — |
| bhv | 27.0 (1.1×) | 186.4 (4.7×) | 0 | 0 | 16 + 2904 | 93 | 3474 | 0 | — |
| behavior-tree-lite | 82.7 (3.3×) | 570.3 (14.4×) | 0 | 0 | 248 + 14392 | 95 | 5480 | 0 | — |

### Instructions and branch mispredictions per tick (one agent)

| library | select8 | patrol | guard | soldier |
|---|--:|--:|--:|--:|
| flatbt | 580 / 7.75 | 104 / 0.55 | 156 / 0.84 | 300 / 4.22 |
| bonsai-bt | 8326 / 85.68 | 283 / 2.77 | 585 / 6.14 | 2603 / 32.63 |
| behavior-tree | 3245 / 33.63 | 448 / 3.22 | 864 / 5.60 | 1572 / 14.72 |
| bhv | 515 / 4.43 | 97 / 0.43 | 141 / 0.65 | 247 / 2.97 |
| behavior-tree-lite | 1951 / 14.75 | 342 / 0.59 | 536 / 3.09 | 838 / 6.11 |

## Reading the results

- **Memory.** A FlatBT agent is its state: 1–3 bytes here, no heap, and one
  tree of 96 bytes to 1 KB serves every agent. The others need 208 bytes
  (`bhv`, `patrol`) to 16.9 KB (`behavior-tree`, `soldier`) per agent, built
  in 6–221 allocations. For 10k soldiers that is 30 KB against 29 MB to
  169 MB.
- **Allocation while ticking.** FlatBT, `bhv` and `behavior-tree-lite` allocate
  nothing. bonsai-bt rebuilds child state from a clone of the `Behavior` on
  every transition and on `reset_bt`: 5 allocations per `soldier` tick, 19
  per `select8` tick, and 3.8 MB of heap growth while 10k soldiers tick.
  behavior-tree builds a debug string in every sequence tick.
- **Many agents.** FlatBT is 2.4–4.7× faster than the next crate, `bhv`, and
  4.4–31× faster than the rest. The gap widens with tree size: on `soldier`,
  going from 1 to 10k agents costs FlatBT 1.6× per tick and the others 3.5–7×,
  because a FlatBT agent's working set is a few bytes while the per-agent
  boxed trees spill out of cache.
- **One hot agent.** `bhv` is level with FlatBT (0.9–1.6× its time) and
  executes 7–18% fewer instructions with fewer mispredictions. With everything
  in L1 and the `dyn` calls predicted, static dispatch buys nothing, and
  FlatBT's control nodes cost more per call. On `soldier`, 55% of FlatBT's
  instructions are in `ControlNode::update` (`src/composition/control.rs`) and
  the child-state dispatch in `src/composition/children.rs`, 14% of them in
  the function's prologue and epilogue alone; the leaves take 10%. That is
  the place to look for single-agent speed.

Not measured: compile time, binary size, reactive (`Evaluate`) ticking,
parallel ticking.
