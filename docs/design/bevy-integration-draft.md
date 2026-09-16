# Bevy integration

Status: implemented as `flatbt-bevy`, re-exported by the `bevy` feature of `flatbt`.

## Goal

Run FlatBT trees on Bevy entities without per-tree or per-node registration, with
world access declared in a form Bevy's scheduler can use.

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
- Structural changes go through `Commands`, never `&mut World` during the tick.
- `bevy_behave`'s stated goal of avoiding `&mut World` systems so the tick
  schedules in parallel with unrelated systems.

What is not reused: an entity per running leaf. FlatBT dispatches statically and
stores invocation state inline, so a leaf can be a plain Rust function over a
typed view of the agent. Reifying it would add the cost without buying anything.

## Constraints

- `BtState<'root, N, C>` borrows the tree. A Bevy component is `'static`, so the
  tree goes in a resource and the component holds the invocation state.
- Tree types are unnameable (closures), but they need never be *named*: both the
  context and the tree type are inferred from the builder function at every site,
  and `impl BehaviorNode<C>` names a subtree in return position.
- `Component: Send + Sync + 'static`. FlatBT only requires `Send` of node state,
  so Bevy trees additionally require `Sync`.
- `BtNode<C>` fixes one context type. The context must therefore carry the
  update's borrows, which makes it a type with lifetimes.

## Design

`BehaviorContext` declares, once per family of trees, what those trees may touch:

- `type Agent: IterQueryData` — the agent's own components. `IterQueryData` is
  Bevy's guarantee that the access is disjoint between entities, which is what
  makes mutable agent state safe to tick in parallel.
- `type Param: ReadOnlySystemParam` — shared world access: resources, lookup
  queries.

Nodes receive `Blackboard<C>`: the agent view (via `Deref`), `shared`, `entity`, and
`commands`. Read-only shared access is a deliberate restriction rather than an
omission: per-entity mutation plus deferred everything-else is the discipline
that makes `Query::par_iter_mut` sound with no further declaration, so the serial
and parallel ticks accept the same trees. Mutating anything outside the agent
goes through `Commands`.

The split follows what is shared and what is not. A tree is an immutable
definition serving many agents, so `BehaviorTree<C, F>` is a resource holding the
one built tree. `Behavior<C, F>` is the component and holds only the saved state of a suspended
invocation, sized exactly for that tree. Everything else about a tick is decided
by the tick, not stored per agent.

Entry mode is the case worth spelling out. It is not a setting: resuming is the
cheap path, and evaluating from the root is a tool aimed at a moment — a timer,
a perception event, a changed order. A per-agent default would make every tree
pay for reactivity it did not ask for, and a marker component asking for one
`Evaluate` would cost two commands per request. So it is
`BehaviorContext::entry_mode`, answered per agent per tick from the access the
context already declares, defaulting to `Resume` and folding away when it is
constant.

Periodic revalidation is the common case of that decision, and it needs no
scheduler: `evaluate_every` derives each agent's slot within the period from its
`Entity` and compares the period boundary against the last tick, so it is exact,
stateless, and spread. Measured over 64 agents at 16 ms ticks, the busiest frame
carries 6 rather than all 64.

A scheduler would only be needed for the harder question, a *budget* -- at most
N revalidations per frame, whoever is most overdue. That needs state shared
across agents and mutated during the tick, which the read-only `Param` and
`par_iter_mut` deliberately rule out, so it would have to be a separate pass
that marks agents before the tick. Left open until something needs it;
staggering removes the spike that motivates it.

Nothing is stored to be looked at either: observation is the tree's own
business, through `Blackboard<C>` and `Commands`. Nothing is erased,
allocated, copied per agent, or reference counted.

Identity is the builder `F`, not the tree it returns. `TreeBuilder<C>` is
implemented for every `Fn() -> impl BehaviorNode<C>`, so a plain function names a
tree. This matters because two builders can return the *same* tree type with
different node configuration: `fn calm()` and `fn angry()` both returning
`impl BehaviorNode<Guard>` around the same nodes share one hidden type, so keying
on the tree type collapsed them into one resource and one system, and every agent
silently ran whichever was registered last. Keying on the builder keeps them
apart, and makes the identity a name the author writes rather than a type the
compiler generated.

`BehaviorPlugin::for_tree(builder)` builds the tree into that resource and
registers `tick_behaviors::<C, F>` in `Update` (or `.in_schedule(..)`),
optionally `.parallel()`. The system declares `Res<BehaviorTree<C, F>>`,
`Query<(Entity, &mut Behavior<C, F>, C::Agent)>` and `C::Param`, so Bevy
schedules it against other systems by real access, and `.parallel()` spreads
agents over the task pool with per-thread command queues.

The cost of builder identity is that a builder has to be spelled the same way at
both sites: `shoot` (a function item) and `shoot as fn() -> _` (a function
pointer) are different names. The mismatch produces a component type no system
queries, which is invisible on its own, so `Behavior`'s `on_add` hook checks that
`BehaviorTree<C, F>` exists and reports it by name. That covers a forgotten
registration too. The function-pointer spelling is also the escape hatch for
writing the component type in a query.

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
all. The hook survives as a diagnostic only, since nothing else can see an agent
whose tree was never registered.

Per-tree systems do not by themselves make trees of one context run concurrently.
Bevy schedules on declared component access rather than on which entities match,
so two trees whose `Agent` writes the same component serialize even though each
agent runs exactly one tree. A read-only `Agent` that routes changes through
`Commands` declares no writes and does overlap — measured as zero conflicting
pairs against one for the mutable form — but defers every change to the next sync
point, which suits observing trees rather than hot per-agent state. Trees over
different contexts always overlap. Parallelism across agents of one tree comes
from `.parallel()`, which needs `bevy_ecs`'s `multi_threaded` feature; the crate
leaves that choice to the consuming app and enables it for its own tests.

## Closure binding

A stored tree must satisfy `for<'w, 's, 'q, 'a, 'c> BtNode<Blackboard<'w, 's, 'q, 'a, 'c, C>>`.
A closure commits to one fixed set of update lifetimes as soon as a constructor
names the context in a bound, and can then never satisfy that. This is not a Bevy
problem but a borrowed-context one, so the fix belongs in core: `check` and
`leaf` take the callable without a bound, leaving the closure open until the
`BtNode` impl is required where the tree runs. `choose!` and `action` were
already like this, which is why they worked from the start.

`compute` behind `scope!` had the same bound, kept so that a `context: Type;`
declaration could type an unannotated initializer. That affordance cost the whole
construct its use from Bevy, and it only ever saved an argument annotation that
`leaf` and `check` already require, so `compute` lost its bound too and
`context:` went with it. The integration now adds no constructors of its own.

## Rejected alternatives

- **`&mut World` context.** One lifetime, no context declaration, nodes may do
  anything. Rejected: an exclusive system cannot overlap with any other system,
  and undeclared access gives the scheduler nothing to work with.
- **Entity per running leaf**, as in the crates above. Rejected: pays spawn,
  despawn, and an observer hop for what is a direct call here.
- **Tree registry keyed by name or handle.** Rejected: the tree is already data;
  an `Arc` in the component needs no second lookup and no lifecycle of its own.
- **Registering the tick from a component hook on first spawn.** Rejected:
  `World::try_schedule_scope` removes the running schedule, so a system added to
  it during its own run is silently discarded. Registration stays explicit.
- **Erasing the root** (`Arc<dyn ErasedTree<C>>` plus a boxed state), so one
  component and one system serve a whole context. This was the first
  implementation. Rejected: erasing the tree erases its state type too, which
  forces the state into a `Box` and gives up the inline state and static dispatch
  the runtime exists to provide. Inference makes the static form no harder to
  write, and `choose!` already selects between subtrees at runtime, which is what
  erasure was mostly buying.
- **Link-time registration** (`inventory` and similar) for an attribute that
  registers a tree where it is defined. Rejected: a proc-macro crate, a
  linker-section dependency, and registrations that vanish under dead-code
  elimination, to replace a hook that costs nothing and cannot be dead-stripped
  because it hangs off the component the agent already carries.
- **Running ticks from a nested schedule**, which a hook could extend freely.
  Rejected: the nested schedule is one opaque system to the outer scheduler, so
  behavior ticks would stop overlapping with unrelated systems.
- **Keying trees by a name string** in one shared registry. Rejected: the state
  type must appear in the component type, so the tree type must too; a name can
  only be an addition to the type, not a replacement, and resolving one at spawn
  time would need world access the constructor does not have. The builder is
  already a name with a type attached.
- **Keeping the tree in the component**, by value or behind an `Arc`. Rejected:
  a tree is an immutable definition shared by every agent running it, so a copy
  per agent duplicates its node configuration and an `Arc` adds a reference count
  and an indirection for something that never changes. A resource says what is
  true: one tree, many agents.
- **A debug report of agents that own a behavior but do not match the agent
  query.** Rejected once that query became a documented gate: the report cannot
  tell a deliberate gate from a forgotten component, so it fires on every
  turn-based or filtered setup. The unambiguous mistake, a tree that was never
  built, is still reported by `Behavior`'s `on_add` hook.
- **Public tick systems and a public `Behavior::tick`**, as an escape hatch for a
  custom driver. Rejected as fiction: `tick_behaviors::<C, F>` cannot be written
  at a call site, because `F` is a builder's own type and nothing there infers
  it. Schedule, ordering, run conditions and the parallel tick are all reachable
  through the plugins, so the hatch only widened the surface.
- **A `BehaviorPaused` component** with a `Blackboard::pause` helper, filtered out of the
  tick. Rejected once it was clear the crate adds nothing: a run condition on
  `BehaviorSystems` already halts every tree, self-registered ticks included,
  and halting one agent is a guard at the root of its tree, which is what a
  behavior tree is for. Archetype-level skipping of a paused subset is the only
  thing lost; if it is ever wanted, it belongs as a `Filter` associated type on
  the context, not as a component the crate owns.
- **A `BehaviorStatus<C>` component** carrying the last result, so systems could
  observe agents whose tree type they cannot name. Rejected: the user's code is
  the tree's nodes, which already read and write the agent through `Blackboard<C>` and
  can signal anything through `bb.commands`. It also added a `&mut` access shared
  by every tree of a context, which would have serialized them beyond what the
  agent components already force.

## Turn based

The design does not assume a frame loop, and the parts fall out of what is
already there. `C::Agent` is a query, so requiring a marker in it ticks exactly
the agents that hold the marker, and the game moves it in whatever order it
keeps; agents out of turn are not iterated. A turn spanning several ticks needs
no extra state, because a suspended invocation is what FlatBT already stores:
the agent resumes where it left off when its turn comes round. Handing the turn
on is the tree's own last act, through `Commands`. When the tick runs is
`in_schedule` or a run condition.

The one consequence is that a deliberately gating agent query is
indistinguishable from a forgotten component, which is why there is no
diagnostic for it.

## What the integration still carries, and why

Roughly 1100 lines including docs and examples; about 400 of code. Attribution,
after moving every constructor fix into core and the blackboard to a snapshot:

| Piece | Forced by |
| --- | --- |
| `BehaviorContext`, `Blackboard` | Declaring ECS access, and gathering a per-agent view from it. This is the integration. |
| `read` / `write` | The snapshot. Their cost is two functions per context; what they buy is that nothing below them knows the ECS exists. |
| `BehaviorTree` resource, `Behavior` component, `TreeBuilder` | Bevy resources and components are `'static`, and the state type has to be nameable without naming the tree. |
| Plugins, tick systems | Bevy scheduling. |
| `BehaviorNode<C>` | FlatBT: `Behavior` needs the invocation state as one named type, and `State: Bound` leaves a projection while `State = T` needs a nameable `T`, which a composed tree's state is not. An associated type is the only equality target left, so it takes a trait — and in return position that trait also names a subtree without naming its type. |

Only `BehaviorNode` is FlatBT's shape, and with the snapshot it is a plain bound
rather than a higher-ranked one. The `AgentAction` wrapper written to hide the
old five lifetimes was deleted the day after it was added; a GAT-based context
trait, considered for the same reason, is likewise unnecessary now.

## What a real game would settle

This design was written without a consumer, and it shows: each review round
removed something that had looked necessary -- a status component, an erased
tree, a pause component, a public tick, two policy flags. That pattern is the
finding. The requirements below came from the first look at a real game, and
they do not point at polish.

### Live borrows were the choice everything else followed from — and are gone

`Blackboard<C>` used to hold the update's borrows. Five lifetimes, the
higher-ranked bound, the shape of `BehaviorNode`, and the constructor changes in
core were all downstream of that one decision, and so was the ceiling on what
the integration could do: a node holding live borrows cannot outlive the system
run, so a tree could never leave the frame thread.

It now holds a **snapshot**: `BehaviorContext::read` gathers a plain struct
before the tick and `write` puts it back after. Every consequence reversed.
`Blackboard<C>` has no lifetimes, so a node signature names nothing but
`Blackboard<Guard>` and an action implements `BtAction<Blackboard<Guard>>`
directly. `BehaviorNode` collapsed to an ordinary bound with one associated
type. The `AgentAction` wrapper added to hide the lifetimes was deleted the same
day. `params` work again, so `scope!` composes. And a tree is a value that takes
a value, which `games/arena/tests/trees.rs` exercises with no `World` at all.

It cost nothing measurable. At 100 000 agents the arena ticks in 4.1 ms serial
and 1.9 ms parallel, against 4.2 / 1.9 for the borrowed version -- but only
after the per-agent `CommandQueue` was made lazy. Owning an empty one per agent
per tick cost a fifth of the whole tick, because dropping a `CommandQueue` walks
its buffer whether or not anything is in it.

What remains true: an action still cannot run an arbitrary query. The declared
access is what buys scheduling and `par_iter_mut`, so reaching past it has to go
through the world — a request a system answers, read back on a later tick.

What is unlocked but not built: the three steps are separable, so the tick could
run on a task or over several frames. Today all three run in one system.

### Tree storage

One resource per `(context, builder)` pair, with `TreeBuilder` as the identity,
is what typed state forces: the component must know the state's size, so it must
know the tree's type. A single resource holding trees in named fields -- the
shape that reads simplest -- needs the root erased, which an earlier round
removed on purpose. The two preferences are in tension and only a consumer can
say which matters more.

### Registration

Self-registration is the largest and least obvious part of the crate, and it
exists to save one line per tree. It also carried the one blocking bug found in
review. If a game finds explicit registration unobjectionable, that machinery
should go.

## Open questions

Closed since the arena:

- **Whether `entry_mode` wants to vary per agent.** It does, and it is free:
  `read` runs per agent, so the answer can be a field. The arena decides
  `rethink` there rather than carrying the clock into every node.
- **Whether the blackboard should be borrows or a snapshot.** Snapshot, at no
  measurable cost.
- **`write` running every tick.** It does not any more. `Blackboard` sets a flag
  in `DerefMut`, so a tree that only read is never written back and never marks
  its agent changed — Bevy's own bargain, where taking `&mut` counts whether or
  not anything changed. Free in the arena, where almost every agent moves.
- **Mutable shared access.** Not wanted. `Param` is read-only, the agent's own
  data is the mutable part, and anything further is a deferred edit. Allowing
  more would mean a second context flavour for the serial tick alone.
- **Ticking outside `Update`.** Every stage of `Main`, a `FixedUpdate` running
  five times in one frame, and a schedule the game runs itself all hold a tick,
  and deferred edits are applied by whichever schedule ran it. See
  `crates/flatbt-bevy/tests/schedules.rs`.

Still open:

- **Running the tick off the frame thread.** Gather, run and write back are
  separable and the snapshot borrows nothing, so it is no longer forbidden —
  but all three still run in one system. Nothing has asked for it yet.
- **A `Running` branch below a failed resume holds priority down for good.**
  `Behavior::tick` re-enters with `Evaluate` when a resumed update *fails*, but a
  fallback that succeeds or runs hides the failure from it. Only `entry_mode`
  recovers that shape. Accepted: the alternative is `Resume` meaning two things
  inside `select`, which was tried and reverted.
- **Two trees of the same Rust type cannot both be registered**, since the
  resource is keyed by the builder type, and a builder parameterized at runtime
  therefore configures nothing. Making configuration first-class would mean
  keying on the *tree* type and storing configured instances beside it, with
  `Behavior` carrying an index. Measured incentive: splitting one tree across
  three names costs about 6% serially and 10% parallel over 100 000 agents
  (`SPLIT=1 cargo run --release --bin bench` in the arena), because each split
  is its own archetype, system and set of `par_iter` batches. More variants can
  only make that worse, though that was not measured.
- **Batch size control for the parallel tick.** Bevy's default is one batch per
  thread; a tree whose cost varies a lot per agent would want to say otherwise.
- **A revalidation budget per frame.** Staggering smooths the load but sets no
  ceiling.
- **A forgotten `BehaviorPlugin` is a runtime warning, not a compile error**,
  and a tree registered into a schedule nobody runs is silent. Accepted: a
  component means nothing without a system, which is true of Bevy generally.
- **The windowed arena has never been run.** It builds; this container has no
  display.
