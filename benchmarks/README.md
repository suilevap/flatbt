# Comparison with other Rust behavior tree crates

A standalone crate, outside the workspace, that runs the same trees on FlatBT
and four other crates and measures time, instructions, allocation and memory.

```sh
cd benchmarks
cargo run --release                      # full run, ~2 minutes
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

Every library runs the same three trees over the same blackboard and the same
leaf function, [`Op::run`](src/common.rs). A tick advances a small
deterministic world, then ticks the tree once. After 100k ticks each library's
blackboard and Success/Failure/Running counts must equal FlatBT's; all do.

| Scenario | Tree | Shape of the work |
| --- | --- | --- |
| `select8` | `select` of 8 × `seq(is_mode(i), act(i))` | Completes every tick; condition scan with an unpredictable winner |
| `patrol` | `seq(move_to(8), act, move_to(-8), act)` | Running 97% of ticks; resumes the active child |
| `guard` | `select(seq(low_hp, move_to(0), heal), seq(enemy, attack), patrol)` | Three levels; mostly resuming, some rescans |

Choices that keep the trees equivalent:

- Controls keep their place (non-reactive). FlatBT ticks with `EntryMode::Resume`.
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
`codegen-units = 1`. Wall-clock varied by about 5% between full runs; ratios
are against FlatBT.

### select8

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 41.0 | 40.2 | 0 | 0 | 2 + 0 | 0 | 30 | 0 | 128 |
| bonsai-bt | 1129.7 (27.5×) | 1539.2 (38.3×) | 19.00 | 1628 | 112 + 2112 | 21 | 2306 | 1152 | — |
| behavior-tree | 307.0 (7.5×) | 1196.1 (29.8×) | 5.50 | 44 | 104 + 5760 | 81 | 5316 | 16 | — |
| bhv | 39.0 (0.9×) | 122.9 (3.1×) | 0 | 0 | 16 + 800 | 34 | 1077 | 0 | — |
| behavior-tree-lite | 181.4 (4.4×) | 672.4 (16.7×) | 0 | 0 | 248 + 5072 | 35 | 2154 | 0 | — |

### patrol

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 7.1 | 7.8 | 0 | 0 | 1 + 0 | 0 | 8 | 0 | 32 |
| bonsai-bt | 18.4 (2.6×) | 24.2 (3.1×) | 0.06 | 6.5 | 112 + 376 | 3 | 206 | 216 | — |
| behavior-tree | 45.5 (6.4×) | 82.3 (10.6×) | 1.00 | 8 | 104 + 976 | 17 | 672 | 8 | — |
| bhv | 6.8 (1.0×) | 14.7 (1.9×) | 0 | 0 | 16 + 128 | 6 | 256 | 0 | — |
| behavior-tree-lite | 31.0 (4.4×) | 61.7 (7.9×) | 0 | 0 | 248 + 528 | 6 | 355 | 0 | — |

### guard

| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | build allocs | build ns | peak heap growth | shared tree |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| flatbt | 9.8 | 15.9 | 0 | 0 | 2 + 0 | 0 | 9 | 0 | 72 |
| bonsai-bt | 53.6 (5.5×) | 165.0 (10.4×) | 0.49 | 47.8 | 112 + 1192 | 11 | 659 | 61632 | — |
| behavior-tree | 72.9 (7.4×) | 368.7 (23.1×) | 2.07 | 16.5 | 104 + 2892 | 43 | 1483 | 16 | — |
| bhv | 9.4 (1.0×) | 48.1 (3.0×) | 0 | 0 | 16 + 392 | 17 | 655 | 0 | — |
| behavior-tree-lite | 46.4 (4.7×) | 213.7 (13.4×) | 0 | 0 | 248 + 2056 | 17 | 1218 | 0 | — |

### Instructions and branch mispredictions per tick (one agent)

| library | select8 | patrol | guard |
|---|--:|--:|--:|
| flatbt | 582 / 5.75 | 121 / 0.41 | 175 / 0.57 |
| bonsai-bt | 8187 / 82.38 | 259 / 2.70 | 558 / 5.97 |
| behavior-tree | 3204 / 39.38 | 434 / 2.99 | 849 / 5.20 |
| bhv | 469 / 4.43 | 82 / 0.42 | 126 / 0.66 |
| behavior-tree-lite | 1885 / 14.75 | 325 / 0.59 | 518 / 3.09 |

## Reading the results

- **Memory.** A FlatBT agent is its state: 1–2 bytes here, no heap, and one
  tree of 32–128 bytes serves every agent. The others need 144 bytes
  (`bhv`, `patrol`) to 5.8 KB (`behavior-tree`, `select8`) per agent, in 6–81
  allocations.
- **Allocation while ticking.** FlatBT, `bhv` and `behavior-tree-lite` allocate
  nothing. bonsai-bt rebuilds child state from a clone of the `Behavior` on
  every transition and on `reset_bt`: 19 allocations per `select8` tick.
  behavior-tree builds a debug string in every sequence tick.
- **Many agents.** FlatBT is 1.9–3.1× faster than the next crate, `bhv`, and
  3–38× faster than the rest. Going from 1 to 10k agents costs FlatBT at
  most 1.6× per tick and the others up to 5×: a FlatBT agent's working set is
  a few bytes, while the per-agent boxed trees spill out of cache.
- **One hot agent.** `bhv` matches FlatBT on time and executes 19–32% fewer
  instructions. Everything fits in L1 and the `dyn` calls are predicted, so
  FlatBT's static dispatch buys nothing here, while its control nodes cost
  more per call. In `patrol`, about a third of FlatBT's instructions are the
  prologue of `ControlNode::update` (`src/composition/control.rs`), and more
  go to switching the active child's state variant (`src/composition/children.rs`).
  That is the place to look for single-agent speed.

Not measured: compile time, binary size, reactive (`Evaluate`) ticking,
parallel ticking.
