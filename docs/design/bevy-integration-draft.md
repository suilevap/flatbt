# Bevy integration

Status: implemented as `flatbt-bevy`, re-exported by the `bevy` feature of `flatbt`.

## Goal

Run FlatBT trees on Bevy entities without per-tree or per-node registration, and
without the integration deciding anything the game should decide.

## How other Bevy behavior crates do it

| Crate | Tree | Leaf | Per-leaf work for the user |
| --- | --- | --- | --- |
| `bevy_behave` | `ego_tree` of `Behave` values in a `BehaveTree` component | Spawns an entity carrying the user's task component plus `BehaveCtx`; status is reported back through a trigger observed by one global observer | Component + system querying it + explicit `ctx.success()`/`ctx.failure()` |
| `big-brain` | `Thinker` builder component; scorers and actions are child entities | Entity with `ActionState`; user systems match on the state machine | Component + derive + system + registration in a `BigBrainSet` |
| `beet_flow` | Entity tree | Observers propagate control flow between entities | Component + observer |

They converge because their trees are dynamic values: a node cannot name a Rust
function with typed world access, so the leaf is reified as an entity and the
behavior moves into an ordinary system. The costs are the same in each: a spawn
and despawn (or an observer hop) per task, indirection between deciding and
doing, and boilerplate proportional to the number of leaves.

What is worth reusing, and is reused here:

- The tree is a component on the agent entity, not a side table.
- Registration is a plugin, matching `add_event`/`init_resource` habits.
- A public system set so game systems can order against the tick.
- `bevy_behave`'s stated goal of avoiding `&mut World` systems so the tick
  schedules in parallel with unrelated systems.

What is not reused: an entity per running leaf. FlatBT dispatches statically and
stores invocation state inline, so a leaf can be a plain Rust function over a
typed view of the agent. Reifying it would add the cost without buying anything.

## Constraints

- `BtState<'root, N, C>` borrows the tree. A Bevy component is `'static`, so the
  tree goes in a resource and the component holds the invocation state.
- Tree types are unnameable (closures), but they need never be *named*: both the
  blackboard and the tree type are inferred from the builder function at every
  site, and `impl BehaviorNode<C>` names a subtree in return position.
- `Component: Send + Sync + 'static`. FlatBT only requires `Send` of node state,
  so Bevy trees additionally require `Sync`.
- `BtNode<C>` fixes one blackboard type per family of trees.

## Design

**The blackboard is an ordinary component.** That is the whole shape. A tree
over `C` is ticked by a system whose query is `(&mut Behavior<C, F>, &mut C)`,
and nothing in the crate says what is in `C`, how it got there, or what a value
written into it means.

Three systems, in the order Bevy already states:

```rust,ignore
app.add_systems(Update, gather.before(BehaviorSystems))
   .add_plugins(BehaviorPlugin::for_tree(guard_tree))
   .add_systems(Update, carry_out.after(BehaviorSystems));
```

The blackboard has two halves and they are not the same data. The top half is
gathered from the world. The bottom half is what the tree decided, and it is
deliberately not a copy of the top: a tree that moved `post` itself would be
deciding how far an agent walks in a tick, and one that subtracted a round would
be deciding what a shot costs. A tree says what it wants; a system owns what
that means. Both halves live in one component so the tick's query stays two
terms, disjoint per entity, which is what makes `par_iter_mut` sound with no
further declaration.

The split of what is stored follows what is shared and what is not. A tree is an
immutable definition serving many agents, so `BehaviorTree<C, F>` is a resource
holding the one built tree. `Behavior<C, F>` is the component and holds only the
saved state of a suspended invocation, sized exactly for that tree. Nothing is
erased, allocated, copied per agent, or reference counted.

What a tick does with one agent is decided per agent per tick by
`fn(&C) -> Tick`, given to the plugin. `Tick` is `Evaluate`, `Resume` or `Skip`;
it defaults to `Evaluate` and folds away when it is constant. Evaluating is the
default because it is the answer that cannot be wrong: a tree that only ever
resumes never leaves the branch it is in, so `select` never rescans and
`choose!` never re-picks. Resuming is an optimisation, correct exactly when the
standing decision is known to still hold — and worth measuring before reaching
for, since a tree whose invocations end each tick has nothing to resume into.
Because the answer comes from the blackboard, anything it needs — a clock, a
staggered slot, a perception flag, whose turn it is — is gathered like
everything else.

`Skip` is the one that had to be added rather than derived, because a tree
cannot stop itself. Three shapes were tried and all three fail, each pinned in
`crates/flatbt-bevy/tests/entry.rs`:

- A guard as child zero of a `seq`, under `Resume`: never consulted again, since
  `Resume` re-enters the active child directly and a child above it was not
  looked at this update.
- The same guard under `Evaluate`: also never consulted, because `seq` continues
  its active child on `Evaluate` rather than rescanning.
- A guard under a `select`, which does rescan on `Evaluate`: consulted, but a
  candidate that *fails* leaves the standing branch in place and runs it.
  Failing a candidate is how a tree redirects, not how it stops.

So "do not run this agent" has to be said before the tree is entered. It matters
when the work is elsewhere: an agent walking somewhere the tree already chose,
or waiting out an action a system is carrying out, has nothing to decide until
that ends. Over 200 000 agents with nine in ten idle, serial tick: 2.19-2.24 ms
if every agent enters the tree, 1.49-1.50 ms if nine in ten fail a root guard,
0.62-0.63 ms if nine in ten are skipped — and the guard row is optimistic,
covering only the case where a guard works at all. In `games/arena`, where
skipping covers fighters walking to a spot the tree already chose, `NOSKIP=1`
against the default over 100 000 agents measures 1.28-1.41 ms against
0.72-0.78 serial and 2.31-2.45 against 1.67-1.80 for the whole frame.

Periodic revalidation is the common case of that decision, and it needs no
scheduler: `evaluate_every` derives each agent's slot within the period from its
`Entity` and compares the period boundary against the last tick, so it is exact,
stateless, and spread. Measured over 64 agents at 16 ms ticks, the busiest frame
carries 6 rather than all 64.

A *budget* — at most N revalidations per frame, whoever is most overdue — is the
harder question, and it is now plainly the game's: it needs state shared across
agents, so it belongs in a gather pass that marks agents before the tick.
Staggering removes the spike that motivates it.

Identity is the builder `F`, not the tree it returns. `TreeBuilder<C>` is
implemented for every `Fn() -> impl BehaviorNode<C>`, so a plain function names a
tree. This matters because two builders can return the *same* tree type with
different node configuration: `fn calm()` and `fn angry()` both returning
`impl BehaviorNode<Guard>` around the same nodes share one hidden type, so keying
on the tree type collapsed them into one resource and one system, and every agent
silently ran whichever was registered last. Keying on the builder keeps them
apart, and makes the identity a name the author writes rather than a type the
compiler generated.

The cost of builder identity is that a builder has to be spelled the same way at
both sites: `shoot` (a function item) and `shoot as fn() -> _` (a function
pointer) are different names. The mismatch produces a component type no system
queries. There is no diagnostic for it, because there is nothing to hang one on
any more: the tick query is the blackboard and the state, so an agent that
matches neither is not distinguishable from an agent the game deliberately left
out. A component means nothing without a system, here as anywhere in Bevy.

`BehaviorPlugin::for_tree(builder)` builds the tree and adds its tick when the
app is built. One line per tree, and no ordering to satisfy: the system exists
before any agent does, so any schedule holds it, including one the game runs
itself.

Self-registration was tried and removed. `Behavior`'s `on_add` hook built the
tree and queued its tick system, and a plugin applied those registrations from
an earlier schedule, because a schedule cannot be extended while it runs. It
saved one line per tree and cost the largest and least obvious part of the
crate, a build that could run several times before its command landed, and a
blocking bug: a tick in the same schedule as the registration never landed at
all.

Per-tree systems do not by themselves make trees run concurrently. Bevy schedules
on declared component access rather than on which entities match, so two trees
over the same blackboard serialize even though each agent runs exactly one tree.
Trees over different blackboards always overlap, as does a tick with any
unrelated system. Parallelism across agents of one tree comes from `.parallel()`,
which needs `bevy_ecs`'s `multi_threaded` feature; the crate leaves that choice
to the consuming app and enables it for its own tests.

## What the integration carries, and why

About 210 lines of code, and about 540 with its documentation. Attribution:

| Piece | Forced by |
| --- | --- |
| `BehaviorTree` resource, `Behavior` component, `TreeBuilder` | Bevy resources and components are `'static`, and the state type has to be nameable without naming the tree. |
| `BehaviorNode<C>` | FlatBT: `Behavior` needs the invocation state as one named type, and `State: Bound` leaves a projection while `State = T` needs a nameable `T`, which a composed tree's state is not. An associated type is the only equality target left, so it takes a trait — and in return position that trait also names a subtree without naming its type. |
| Plugins, tick systems | Bevy scheduling. |
| `evaluate_every` | Nothing. It is a policy, and it is a free function a game could have written; it ships because staggering is easy to get subtly wrong. |
| `Tick::Skip` | Nothing in Rust; it is here because a tree cannot express it, as above. |
| `Split<In, Out>` | Nothing. Forty lines a game could write, and it ships only because private fields cross a module boundary and a hand-rolled version in the game's own module would not. |

Everything else is the game's. `Behavior::tick` is public, so a game that wants
its own tick system — a different query, its own parallel strategy, a tick that
does not run in a system at all — writes one without forking the crate, and the
only thing it cannot write for itself is the generic that names the tree.

## The road here: three shapes of blackboard

This is worth recording because each step looked necessary at the time and each
one was removed by looking at a real consumer.

**1. Live borrows.** `Blackboard<'w, 's, 'q, 'a, 'c, C>` held the update's
borrows. Five lifetimes, a higher-ranked `BehaviorNode` bound, a constructor
change in core so closures could stay open, and a ceiling on what the
integration could do. `params` did not work, so `scope!` did not compose.

**2. A snapshot, declared by a context trait.** `BehaviorContext` named the ECS
access as `type Agent: IterQueryData` and `type Param: ReadOnlySystemParam`;
`read` gathered a plain struct before the tick and `write` put it back after.
Every lifetime consequence reversed: node signatures named `Blackboard<Guard>`,
`BtAction<Blackboard<Guard>>` was written directly, `scope!` composed, and a
tree became a value that takes a value. The core constructor change stopped
being load-bearing here -- it had been forced by shape 1, and shape 2 could have
lived without it. It is on `main` regardless: removing a constructor bound costs
a plain context nothing and leaves an inline closure usable at any update.

**3. A plain component.** The snapshot was still a stack temporary produced by
`read`, and that is what did not survive contact:

- **A real gather is not one function.** Visibility wants a raycast, cover wants
  a path query, and neither should run for an agent that is not in combat.
  `read` is one function at one rate for all agents; expressing "this field, at
  a tenth of the rate, for these agents only" inside it is not possible. As
  ordinary systems it is `.run_if(..)` and a second query.
- **`write` cannot know the output format.** What a tree decides is however that
  game controls its agents — an intent component, a button press, a queued turn
  order — and a library that guesses adds an abstraction the game routes around.

Making the snapshot a component answers both, and answers them by deletion: the
gather is systems, the apply is systems, and the crate holds neither. Its source
went from 436 lines of code to 208. Measured back to back over 100 000 agents in
`games/arena`, the tick is about 1.9x cheaper serially and 2.4x in parallel, and
the whole frame 18% and 8%, because the blackboard is read and written in place
instead of gathered into a temporary per agent and applied back.

What was given up, listed so it is a decision and not an oversight:

- **A node cannot defer a world edit.** There is no `bb.commands`. A tree writes
  its decision to the blackboard and a system carries it out — which the arena
  wanted anyway, since "spawn a projectile" was never the tree's to size, aim or
  own. A tree that genuinely needs an unbounded edit writes a request field and
  a system answers it. See *Commands, and what they cost* below.
- **No warning for an unregistered tree.** See above: there is nothing left to
  hang it on, and Bevy does not warn about a component with no system either.
- **The tick mode no longer sees the `Entity`.** It takes `&C`. A gather that
  needs the entity has it, so a staggered slot or a turn flag is a field like
  any other.
- **The agent query is no longer a gate.** With `type Agent` gone, a turn-based
  game cannot put `With<Turn>` in the tick's query to iterate exactly the
  holder. It gates with `Tick::Skip` instead — the gather writes `has_turn`, the
  tick mode reads it — so out-of-turn agents cost one predicate each rather than
  nothing, and, unlike a guard inside the tree, this also holds across a turn
  that spans several ticks. See `crates/flatbt-bevy/tests/schedules.rs`.

## The tick's one piece of behavior

`Behavior::tick` does one thing beyond calling the tree: when a tick entered as
`Resume` fails at the root, it re-enters once as `Evaluate`.

This is a fix for a real hole. A resumed child that fails hands off to the next
child *below* it, never back to one above, because `Resume` skips `begin()` and
the children above were never consulted this update. So an agent whose standing
decision runs out would otherwise do nothing at all that tick, even though a
higher-priority branch was available. Re-entering as `Evaluate` consults them.

It lives in the tick and not in `select` on purpose. Making a resumed child's
failure rescan from the top inside the control node was implemented (as
`BtControl::continuation_failed`) and reverted: it makes `Resume` mean two
things, and the point of `Resume` is that it is an honest resume from the same
place. One retry at the root is the caller asking for reconsideration, which is
exactly whose decision it is. Pinned by
`crates/flatbt-core/tests/resume.rs` and
`crates/flatbt-bevy/tests/behavior.rs`.

## Keeping the input read-only

The tick takes `&mut C`, so a blackboard is mutable in full and "the tree only
reads the top half" is a convention. `Split<In, Out>` makes it a rule: `In` is
reachable by `Deref` and nothing else, writing goes through `out()`, and both
fields are private, so the rule crosses the module boundary rather than resting
on a comment. Its `compile_fail` doctest is the proof.

It costs nothing measurable — one component, the same two-term query, the same
layout — and every constructor FlatBT ships composes over it unchanged, which
`crates/flatbt-bevy/tests/split.rs` exercises node by node. What it does not do
is tell a gather system from a node: the gather needs `&mut In`, and no Rust
type can distinguish the caller, so `sensed_mut()` is public. The guarantee is
that no node reaches the input by accident.

It is optional on purpose. A blackboard with no meaningful split reads better as
a plain component with flat field names, and `Split` is generic, so it appears
in query types.

Considered and rejected for the same job:

- **Two components**, `&C` and `&mut O` in the tick's query. This is the shape
  the ECS suggests, and it cannot be had: `BtNode<C>` fixes one context type, so
  a node seeing both means a struct holding two borrows, which brings back the
  higher-ranked bound that shape 1 was abandoned for.
- **The output as the root's `params`.** `BtNode<C, P>` already carries `P` down
  through every control node, and `P` is documented as the place for
  update-local borrows, so the mechanism works — a `&mut Out` does reach a leaf.
  It fails on two counts: `update` and `BtState` in core fix the root's `P` to
  `()`, so it needs a core change; and `leaf` and `check` are generic over `P`
  and ignore it, so the two most common nodes cannot reach the output at all and
  every write becomes a hand-written `BtNode` naming both types. `scope!` also
  owns `P` for its locals, so the two would collide inside a scope.

## Commands, and what they cost

`Commands` borrows the world, so a node holding one brings back the lifetimes.
`CommandQueue` does not: it is an owned buffer, so it can simply be a field of
the blackboard, and a system after the tick drains every agent's queue into the
world. That is about twenty lines, works today with nothing from this crate, and
`crates/flatbt-bevy/tests/commands.rs` is a working copy kept as the record.

What it costs, over 200 000 agents, serial tick / whole frame:

| blackboard | tick | frame |
| --- | --- | --- |
| a `bool` field, carried out by a system | 0.77-0.84 ms | 0.94-1.00 ms |
| a queue nothing writes to | 1.30-1.33 ms | 1.90-1.98 ms |
| a queue 1 agent in 100 writes to | 1.40-1.46 ms | 2.16-2.26 ms |
| a queue every agent writes to | 4.82-4.84 ms | 19.2-19.4 ms |

Carrying an unused queue costs 70% of the tick: the blackboard grows by 56 bytes
per agent, and the drain is another pass over the population. Using one
everywhere costs twenty times the frame. So a queue suits the rare structural
edit — a spawn, a despawn, an archetype move, a handful per frame — and never
what an agent decides every tick, which is a field and a system.

This is why the crate ships no command channel rather than an opt-in one: an
opt-in the game can write in twenty lines, whose cost depends entirely on how
the game uses it, is better as a documented pattern with a price than as an API
that looks free.

## Asking the world

A tree reads and writes its blackboard and nothing else, so reaching past it
means asking: write the question where the gather will see it, and wait. `ask`
is that as one node — `request` runs once per invocation, `is_in_progress` holds
until `answered` returns a value — and bound to a `scope!` output slot it hands
the answer on as a plain value, so the node after it takes a `Vec2` rather than
an `Option<Vec2>` and cannot run without one.

It lives in `flatbt-nodes`, not here. The earlier version inserted a marker
component and read a `CoverTarget` back, which made it Bevy's; with the question
and the answer both fields, `ask` is two closures over `C` and knows nothing
about the ECS. An ordinary system is what connects the fields to the world.

`Request<T>` puts both in one field — `Idle`, `Pending`, `Answered(T)` — so a
question costs one field rather than two loose flags, and the system answering
it matches on `Pending` instead of re-deriving *when* an answer is wanted. That
condition then lives once, in the tree that decided it.

### Why a node cannot simply run the query itself

The obvious objection to all of this is that `ask` does not find anything: a
system does, and `ask` only writes the question down. So why can a node not hold
the `Query` and answer for itself?

Because a tree outlives a frame and a `Query` does not. The tree is built once
into a resource, so it is `'static`; a `Query<'w, 's, ..>` borrows the world for
one system run. Three ways around that were written and all three fail, for
reasons that are the language's rather than this crate's:

- **Through the context.** `C` would have to hold the borrows. That is shape 1
  above, with the higher-ranked bound and everything downstream of it.
- **Through `params`.** `P` is documented as the place for update-local borrows
  and does reach a leaf. But `ParamValue` is implemented for `&T` and `&mut T`
  with `T: 'static`, and a query behind a reference is `&'a Query<'w, 's, ..>` —
  two levels of lifetime. `for<'a, 'w, 's>` over that does not resolve:
  *"implementation of `BtNode` is not general enough"*, in a five-line probe
  with no Bevy in it at all. Passing the borrowing type by value instead fails
  earlier, since `ParamValue` has no impl for it.
- **Through a trait object**, `&'a dyn Perception`, which would hide the query's
  own lifetimes behind the one the tree quantifies over. This is the shape that
  should work, and it is blocked by `T: 'static` and `T: Sized` on the `&T` impl
  of `ParamValue`. Relaxing those means `Value<'a> = &'a T where T: 'a`, which
  puts a lifetime bound back into the parameter machinery for every tree.

So the split is not a workaround: a tree that persists cannot hold a borrow that
does not, and the blackboard is where the two meet. What `ask` adds is that the
meeting is written once per invocation rather than once per tick, and that the
answer leaves the blackboard for a scope local as soon as it arrives.

A `dyn`-based escape hatch is the one worth revisiting if a consumer needs it,
since it is a bounded change to `ParamValue` rather than a return to shape 1.
Nothing has asked for it yet.

The shape matters: a leaf returning `Running` is re-entered on every resume, so
a leaf that asks asks again every tick. As an action, `start` runs once per
invocation, which is what asking means. That was the arena's 220 ns-per-agent
bug before it was a node.

## Rejected alternatives

- **`&mut World` context.** One lifetime, no declaration, nodes may do anything.
  Rejected: an exclusive system cannot overlap with any other system, and
  undeclared access gives the scheduler nothing to work with.
- **Entity per running leaf**, as in the crates above. Rejected: pays spawn,
  despawn, and an observer hop for what is a direct call here.
- **`BehaviorContext` with `read`/`write`.** Shape 2 above. Rejected by the two
  findings in "The road here": a gather is several systems at several rates, and
  an output format is the game's.
- **Keeping `read`/`write` as an optional convenience** beside the plain tick.
  Rejected: two ways to do the same thing, where the one that reads shorter is
  the one that stops working as a game grows.
- **A command channel in the tick**, as an opt-in flag on the plugin. Rejected
  on measurement: see *Commands, and what they cost*.
- **A `Skip` expressed as a guard inside the tree.** Rejected on semantics: no
  entry mode consults a root guard once the tree is suspended below it.
- **Tree registry keyed by name or handle.** Rejected: the tree is already data;
  a resource keyed by the builder type needs no second lookup.
- **Registering the tick from a component hook on first spawn.** Rejected:
  `World::try_schedule_scope` removes the running schedule, so a system added to
  it during its own run is silently discarded. Registration stays explicit.
- **Erasing the root** (`Arc<dyn ErasedTree<C>>` plus a boxed state), so one
  component and one system serve every tree over a blackboard. Rejected: erasing
  the tree erases its state type too, which forces the state into a `Box` and
  gives up the inline state and static dispatch the runtime exists to provide.
- **Link-time registration** (`inventory` and similar). Rejected: a proc-macro
  crate, a linker-section dependency, and registrations that vanish under dead
  code elimination, to replace one line per tree.
- **Running ticks from a nested schedule.** Rejected: the nested schedule is one
  opaque system to the outer scheduler, so behavior ticks would stop overlapping
  with unrelated systems.
- **Keeping the tree in the component**, by value or behind an `Arc`. Rejected:
  a tree is an immutable definition shared by every agent running it. A resource
  says what is true: one tree, many agents.
- **A `BehaviorPaused` component** filtered out of the tick. Rejected: a run
  condition on `BehaviorSystems` halts every tree, and halting one agent is a
  guard at the root of its tree, which is what a behavior tree is for.
- **A `BehaviorStatus<C>` component** carrying the last result. Rejected: the
  blackboard is the tree's output, and it is the game's to shape.

## Open questions

- **Two trees of the same Rust type cannot both be registered**, since the
  resource is keyed by the builder type, and a builder parameterized at runtime
  therefore configures nothing. Making configuration first-class would mean
  keying on the *tree* type and storing configured instances beside it, with
  `Behavior` carrying an index. Measured incentive: splitting one tree across
  three names costs about 6% serially and 10% parallel over 100 000 agents
  (`SPLIT=1 cargo run --release --bin bench` in the arena), because each split
  is its own archetype, system and set of `par_iter` batches.
- **A `Running` branch below a failed resume holds priority down for good.** The
  retry above fires when a resumed tick *fails*; a fallback that succeeds or runs
  hides the failure from it. Only `entry_mode` recovers that shape. Accepted:
  the alternative is `Resume` meaning two things inside `select`.
- **Batch size control for the parallel tick.** Bevy's default is one batch per
  thread; a tree whose cost varies a lot per agent would want to say otherwise.
- **Running the tick off the frame thread.** The blackboard is a component and
  borrows nothing, so a gather at frame N could in principle be ticked later.
  Nothing has asked for it.
- **The windowed arena has never been run.** It builds; this container has no
  display.
