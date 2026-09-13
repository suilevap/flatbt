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
| 10 000  | 0.72 ms | 0.50 ms       | 1.43x   |
| 50 000  | 2.81 ms | 1.24 ms       | 2.27x   |
| 100 000 | 5.65 ms | 2.61 ms       | 2.16x   |
| 200 000 | 11.6 ms | 4.28 ms       | 2.71x   |
| 400 000 | 23.6 ms | 8.23 ms       | 2.86x   |

About 57 ns per agent per tick serially. Below ~5 000 agents the task pool
costs more than it saves and `.parallel()` is a loss, which is why it is opt-in
rather than the default.

## What building it changed

**Commands are handed out per batch, not per agent.** The parallel tick used
`ParallelCommands::command_scope` around each agent. That takes a thread-local
borrow per call, and at the cost of a real tree it ate most of the speedup --
1.6x instead of 2.3x at 100 000 agents. It now builds one `CommandQueue` per
`par_iter` batch and appends them when the pass is over. `bin/scaling` is the
control that measures the difference in isolation.

## What building it found

**A tree that only resumes never changes its mind.** `select` rescans its
children on `EntryMode::Evaluate` and only then; `choose!` re-picks the same
way. The default entry mode is `Resume`, so a reactive tree does nothing
reactive until its context says when to reconsider. The coward needs it to
notice it is hurt, and `take_cover` needs it to notice its request was
answered. `Fighter::entry_mode` uses `evaluate_every`, which also keeps the
population from reconsidering all on one frame.

This was not a small mistake to make. With the trees stuck in the asking
branch, every hurt coward re-inserted its request every frame, and the
resulting archetype churn cost 220 ns per agent per tick -- four times the
whole tree -- while scaling to exactly 1.0x however many threads it was given.
Nothing in the API hinted at it, and the cost showed up as "parallelism does
not work here" rather than as "this tree is wrong".

**Asking the world something is an action, not a leaf.** A leaf that returns
`Running` runs again on every resume, so a leaf that inserts a request inserts
it every frame. `BtAction::start` runs once per invocation, `is_in_progress`
is where the waiting goes, and `complete` is where the answer gets used --
which is exactly the shape of a deferred query. `AskForCover` in `src/ai.rs`
is the whole pattern, and it is nine lines.
