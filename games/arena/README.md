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
| 10 000  | 0.55 ms | 0.47 ms       | 1.18x   |
| 50 000  | 2.09 ms | 1.14 ms       | 1.84x   |
| 100 000 | 4.20 ms | 1.89 ms       | 2.23x   |
| 200 000 | 8.34 ms | 3.72 ms       | 2.24x   |
| 400 000 | 17.1 ms | 6.57 ms       | 2.60x   |

About 42 ns per agent per tick serially, of which roughly 1 ns is the
revalidation guard deciding whether this agent reconsiders at all, and about
8 ns is `select` rescanning priority after a resumed branch fails. Below
~5 000 agents the task pool costs more than it saves and `.parallel()` is a
loss, which is why it is opt-in rather than the default.

## What building it changed

**`select` rescans priority when its resumed branch fails.** A resumed branch
that fails leaves the selector choosing among children it never consulted, so
it took the branch *below* the failure even when a higher-priority one had
become available -- and if that branch went `Running`, the inversion held for
good, because nothing ended to force an `Evaluate`. `BtControl` grew
`continuation_failed` for it; `Sequence` and `Choose` keep the old behaviour,
which was already right for them. It costs about 8 ns per agent per tick here.

**`ask` is now a node the integration ships.** Asking the world something the
context never declared was 25 lines of hand-written `BtAction` per question.
It is one line: `ask(WantsCover, |bb| bb.cover.is_some())`.

**Actions no longer name the blackboard's lifetimes.** `BtAction` is generic
over its context, so implementing it for a blackboard meant writing
`Blackboard<'_, '_, '_, '_, '_, Fighter>` in the impl header and in every
method -- five lifetimes that are an artefact of assembling the blackboard from
borrows and that no action ever mentions. `AgentAction` fixes the context, so
its methods elide them like any ordinary function, and `act` turns one into a
node. The game now spells that signature zero times; the whole integration
spells it once, in the bridge impl.

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
