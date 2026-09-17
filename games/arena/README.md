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
names, to measure what splitting one costs), `NOSKIP=1` (turn `Tick::Skip` off,
to measure what it saves) and `ACTIONS=none|move|all` (how many decisions are
carried out to components, to measure what that costs).

It is its own workspace, so the repository's checks never build Bevy's renderer.

## What is in it

- `src/ai.rs` -- one `Fighter` blackboard, the three systems that fill it, three
  trees (`chaser`, `sniper`, `coward`) over it, and the one system that carries
  out what they decided. The coward switches between the other two behaviours
  with `choose!`, and hides using a query no tree can make.
- `src/world.rs` -- the game's own truth: the components, and the shared
  resource the gather reads.

## Gather, decide, act

`Fighter` is what a fighter knows. What it *decides* is also fields of it,
because a node sees `&mut Fighter` and nothing else -- but those fields are a
detail between the trees and `ai::plugin`, and nothing outside `src/ai.rs` reads
them. Five of them leave as components:

| component | registered as | acted on by |
| --- | --- | --- |
| `MovingTo(Vec2)` | `describing` | `movement` |
| `Firing` | `while_` | `weapons` |
| `Meleeing` | `while_` | `weapons` |
| `Reloading` | `while_` | `weapons` |
| `LookingForCover` | `while_` | `find_cover` |

So no tree here changes the world. `Reload` does not subtract from a magazine
and does not know how long reloading takes: it says "reloading" and waits for
`weapons` to work `reload_left` down to zero. `MoveTo` names a destination and
waits until `movement` has got there. Every action is that shape, which is what
makes them affordable as components -- see the table below.

```rust
fn start(&self, f: &mut Fighter, _: ()) -> Option<()> {
    f.reloading = true;          // not `f.ammo = 6`
    Some(())
}

fn is_in_progress(&self, _: &(), f: &Fighter, _: ()) -> bool {
    f.reload_left > 0            // the world says when it is done
}
```

It reads best in the tests: they assert on what a tree decided
(`move_to == Some(player)`) rather than on consequences (`position.x < 100.0`),
so a failure says which judgement was wrong instead of which number moved. And
because the blackboard is a plain struct, `tests/trees.rs` runs these very trees
with no Bevy app at all.

## The gather is three systems, not one

This is what the arena settled, and it is why the integration does not own the
gather. Filling `Fighter` is not one job at one rate:

| system | rate | what it fills |
| --- | --- | --- |
| `gather_agent` | every tick, `par_iter_mut` | position, health, ammo, speed, the player -- and clears last tick's decisions |
| `gather_pace` | every tick, `par_iter_mut` | `rethink`, this agent's staggered slot |
| `find_cover` | every tenth tick, only for agents that asked | `cover`, by searching the cover query |

`find_cover` is the one that matters. It stands in for a raycast or a path
query: expensive, needed by one branch of one tree, and pointless for an agent
that is not hurt. A single per-agent gather function has nowhere to put "this
field, at a tenth of the rate, for these agents only". Three systems have
`.before(BehaviorSystems)`, a `Local<u32>`, and an `if`.

Asking is an action like any other: `ask` puts the question, it becomes the
`LookingForCover` component, and `find_cover` matches on that.

```rust
scope! {
    let spot: Vec2;
    sequence {
        ask(|f: &mut Fighter| f.out().cover.ask(),
            |f: &Fighter| f.orders().cover.answered().copied()).with(out spot);
        WalkTo.with(spot);
    }
}
```

`WalkTo` takes a `Vec2`, not an `Option<Vec2>`, so it cannot run without one,
and the local belongs to the invocation, so a coward that heals and comes back
asks again rather than walking to a spot picked for an older situation. No
archetype moves and nothing is deferred to a sync point.

The question and its answer are one `Request<Vec2>`. "Who wants cover" therefore
stays in the tree that decided it rather than being re-derived in the gather.
The answer cannot be a `scope!` local until `ask` hands it over, because a local
lives in the invocation state, which is a type no system can name -- and a node
cannot run the cover query for itself, because the tree is `'static` and a
`Query` is not.

## Skipping the agents with nothing to decide

A fighter walking to a spot the tree already chose has nothing to decide until
it arrives -- `carry_out` is what moves it. `pace` says so:

```rust
pub fn pace(fighter: &Fighter) -> Tick {
    if fighter.orders().move_to.is_some() && !fighter.arrived {
        Tick::Skip
    } else if fighter.rethink {
        Tick::Evaluate
    } else {
        Tick::Resume
    }
}
```

A guard inside the tree cannot do this: once the tree is suspended, no entry
mode consults a child above the one it is in. Measured back to back over
100 000 agents, `NOSKIP=1` against the default:

| | AI tick, serial | AI tick, parallel | whole frame, serial | whole frame, parallel |
| --- | --- | --- | --- | --- |
| every agent enters the tree | 1.28-1.41 ms | 0.57-0.62 ms | 2.31-2.45 ms | 1.49-1.59 ms |
| walkers skipped | 0.72-0.78 ms | 0.41-0.44 ms | 1.67-1.80 ms | 1.42-1.50 ms |

The gather also stops clearing a skipped agent's orders, which is the other half
of the same idea: a standing order survives a tick the tree was not asked about.

## Numbers

Three trees over one blackboard, mixed evenly, ticking every frame. Wall time
between `start_timing` and `stop_timing`, which bracket `BehaviorSystems`, on a
4-core Xeon at 2.8 GHz:

| agents  | AI tick | with `.parallel()` | speedup | whole frame | frame, parallel |
| ------- | ------- | ------------------ | ------- | ----------- | --------------- |
| 1 000   | 0.06 ms | 0.14 ms            | 0.45x   | 0.41 ms     | 0.49 ms         |
| 10 000  | 0.16 ms | 0.18 ms            | 0.88x   | 0.84 ms     | 0.87 ms         |
| 50 000  | 0.49 ms | 0.33 ms            | 1.50x   | 2.65 ms     | 2.51 ms         |
| 100 000 | 0.83 ms | 0.52 ms            | 1.61x   | 4.85 ms     | 4.79 ms         |

About 8 ns per agent per tick serially. Most of the frame is no longer the trees
at all: carrying five decisions out to components costs about 0.5-0.8 ms each at
this population, because these fighters change their minds constantly and every
change is an archetype move. `ACTIONS=` takes that apart, over 100 000 agents:

| decisions carried out | whole frame |
| --- | --- |
| `ACTIONS=move` -- one, whose value changes without the component coming and going | 2.53-2.60 ms |
| `ACTIONS=all` -- all five | 4.70-4.91 ms |

That is the bill for the game's own systems being ordinary Bevy, and it is the
worst case: a hundred thousand agents all re-deciding. At a thousand it is tens
of microseconds. A game that cannot afford it leaves the decision a field and
reads the blackboard, which nothing prevents.

One caution about every absolute number here: they were taken on a shared
container whose throughput drifts. The same unchanged binary measured 4.2 ms
over 100 000 agents one afternoon and 2.5 ms the next. Only figures taken back
to back in one session compare — which is what `bin/scaling` is for, and why
the claims elsewhere in this file name an A against a B rather than a
millisecond.

## What building it changed

**Decisions leave as components.** They used to be fields the game read, which
is not an ECS integration -- it is a struct with a schedule around it. Five of
them are components now, and the trees changed shape with them: every action
signals and waits rather than doing, because that is both what an action is and
what makes it affordable. Each registered decision costs about 0.5-0.8 ms of
frame at 100 000 constantly-re-deciding agents, and the alternative is the
blackboard as an interface.

**`Split<In, Out>` came and went.** A blackboard whose input half a node could
not write: it worked, cost nothing, and was removed because enforcing that is
not the framework's business -- and because with the output leaving as
components, the blackboard is input as far as the rest of the game can tell.

**The blackboard became an ordinary component.** It was two other things first:
a bundle of live borrows, then a snapshot gathered by a `BehaviorContext::read`
function and applied by a `write`. The snapshot removed every lifetime from a
node signature; making it a component removed the trait. Two things found here
did that:

- The gather is the table above -- three systems at three rates -- and `read`
  is one function at one rate for every agent.
- `write` had to guess the output format. What a tree decides is however that
  game controls its agents, and the library guessing costs an abstraction the
  game routes around.

Measured back to back at 100 000 agents, snapshot against component:

| | AI tick, serial | AI tick, parallel | whole frame, serial | whole frame, parallel |
| --- | --- | --- | --- | --- |
| snapshot (`read`/`write`) | 2.14-2.39 ms | 1.22-1.25 ms | 2.64-2.97 ms | 1.68-1.71 ms |
| blackboard component | 1.11-1.19 ms | 0.51-0.53 ms | 2.20-2.30 ms | 1.54-1.59 ms |

Three interleaved pairs in one session, same binary, `PLAIN=1` switching between
the two. The tick is about 1.9x cheaper serially and 2.4x in parallel; the frame
follows it by 18% serially and 8% in parallel, which is the honest number -- the
gather did not vanish, it moved into systems that spread across the pool the
same way the tick does.

The tick gets cheaper because the blackboard is read and written in place
rather than gathered into a temporary per agent and applied back.

**Trees decide; systems carry out.** Before this, trees wrote `position`,
`health` and `ammo` directly. Measured back to back at 100 000 agents, that
change made the tick about a tenth cheaper serially and a fifth in parallel
(2.58-2.65 to 2.27-2.46 ms serial, 1.46-1.55 to 1.18-1.22 parallel) and left the
frame unchanged, because what moved out is pure per-entity work that spreads
across the pool as readily as the tick does.

**A tree that fails on resume re-enters from the root in the same tick.** A
resumed update never consults the branches above the one it resumed, so a
failure reached that way says nothing about what the tree would choose now.
`Behavior::tick` retries with `Evaluate`; core keeps `Resume` an honest resume.
It only saves a tick -- and only when the whole tree fails, which a `Running`
fallback below the failure prevents. That case needs `entry_mode`.

**`evaluate_every` no longer divides 128-bit integers.** It ran two of them per
agent per tick, which on a real tree cost more than the tree: 57 ns per agent
against 36 ns at the time. It works in `u64` and takes one remainder instead.

**The parallel tick hands out nothing per agent.** An earlier version wrapped
each agent in `ParallelCommands::command_scope`, which takes a thread-local
borrow per call and at the cost of a real tree ate most of the speedup -- 1.6x
instead of 2.3x at 100 000 agents. Batching the queues fixed it; a blackboard
component removed the need for them entirely. `bin/scaling` is the control that
measures the difference in isolation.

## What building it found

**`Running` is a promise, and a leaf cannot keep it.** `take_cover` asked for
cover with a leaf that returned `Running` while it waited, and asked by
inserting a component directly. Under `Resume` a control node re-runs its active child
directly, so the tree never left that leaf: it re-inserted its request every
frame, and the archetype churn cost 220 ns per agent per tick -- four times the
whole tree -- while scaling to exactly 1.0x however many threads it was given.
The symptom read as "parallelism does not work here", not as "this tree is
wrong".

`Failure` would have been enough on its own. A control node whose resumed
child fails moves on to the next sibling, and an invocation that reaches a
terminal result is dropped, so the next tick starts fresh from the root as
`Evaluate`. Only `Running` sticks. A node that returns it is promising to make
progress and eventually stop. The version here keeps the promise cheaply: the
request is a field, so re-entering the leaf sets the same bool again and moves
no archetype.

**Revalidation is for abandoning work, not for reacting.** With the asking
fixed, the coward switches between fighting and hiding without any entry mode
at all: each invocation ends, and the next one re-picks. `entry_mode` earns its
place only for the branch that does not end -- walking to cover takes a hundred
frames, and a coward healed halfway there should turn around. That costs about
7 ns per agent per tick, and `evaluate_every` spreads it so the population does
not all reconsider on one frame.
