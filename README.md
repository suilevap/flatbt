# FlatBT

Experimental Rust behavior tree runtime. Static dispatch, resumable execution,
inline state. No runtime heap allocation; application-owned state may allocate.
API unstable.

## Install

Packages are unpublished. Use a local checkout:

```toml
[dependencies]
flatbt = { path = "../FlatBT" }
```

Includes core, branch choice, local scopes, and actions by default.
Import the tree authoring API:

```rust
use flatbt::prelude::*;
```

## Run a tree

```rust
use flatbt::prelude::*;

let tree = seq((
    check(|ammo: &usize| *ammo > 0),
    leaf(|ammo: &mut usize| {
        *ammo -= 1;
        NodeResult::Success
    }),
));

let mut state = BtState::new(&tree);
let mut ammo = 1;
assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Success);
assert_eq!(ammo, 0);
```

One immutable tree can serve multiple agents. Each owns a `BtState` bound to that
tree. The tree must outlive its states. `update` rejects a different root.

| Event | State lifetime |
| --- | --- |
| `Running` | Preserve state for the next update. |
| `Success` / `Failure` | Drop invocation state. Next update starts fresh. |
| `state.reset()` or Drop | Drop saved state and descendants. Reset keeps the root binding. |

| Control | Behavior | On `Evaluate` |
| --- | --- | --- |
| `seq((...))` | Run in order; fail on first Failure. Empty sequence succeeds. | Continue the active child. |
| `select((...))` | Try in order; stop on Success or Running. Empty selector fails. | Scan from child zero. |

`Resume` follows the saved path. Fresh invocations always receive `Evaluate`.
A resumed child that fails hands off to the next child below it, never back to
one above: `Resume` skips `begin()`, so the children above were never consulted
this update, and reconsidering them is the caller's to ask for. A caller that
wants it can re-enter with `Evaluate` when a resumed update fails, which is
what [`flatbt-bevy`](crates/flatbt-bevy) does.
A completed child can be followed by another child in the same update.
During revalidation, a failed candidate preserves the old branch; a new Running
candidate replaces it and drops its state.

## Tree and state layout

Full diagram: [tree and state](docs/design/tree-state.md).

- Tree = one nested Rust value: generic controls own tuples of child defs.
  Static dispatch. One tree, many agents.
- Each `BtState` borrows the tree, owns `Option<Tree::State>` inline:
  nested structs/enums, one value, no runtime frames.
- Control = policy state + one-active-child enum. Child size ≈ largest
  alternative + tag/align, not the sum.
- Resume borrows saved state in place. Fresh candidate = temp stack space;
  Running moves in, drops the old branch. User state may heap-alloc.

## Choose a branch

Node definitions are built once, in arm order. Evaluate repeats
the match; Resume keeps the saved arm. The selected result is returned directly,
without fallback.

```rust,ignore
use flatbt::prelude::*;

let tree = choose!(|bb: &Blackboard| match bb.order {
    Order::Move => MoveNode,
    Order::Attack => AttackNode,
    Order::Idle => IdleNode,
});
```

Arm definitions cannot use `bb` or match bindings. Read update-time inputs inside
the node. See the [choice example](examples/choose.rs).

## Share local values

Locals initialize once per invocation and survive suspension
and revalidation. Nodes receive references to explicitly named fields.

```rust,ignore
use flatbt::prelude::*;

let tree = scope! {
    let walk_pos: Vector2 = |bb: &mut World| bb.next_patrol_pos;
    let door_pos: Vector2 = get_visible_door_pos;
    sequence {
        LookAt.with(door_pos);
        wait_frames(1);
        action(Walk).with(walk_pos);
    }
};
```

Initializers are `Fn(&mut World) -> T`, with the argument annotated. `LookAt`
receives `&Vector2`; `Walk`
implements `BtAction<World, &Vector2>`. Use `select { ... }`
for fallback. Nested controls share locals; nested scopes own separate locals.

For a suspending producer, declare `let cover: Vector2;` and bind
`ChooseCover.with(enemy, out cover);`. It receives `(&Enemy, &mut Option<Vector2>)`.
Missing inputs log a diagnostic and fail the consumer. Shared-local writes survive
candidate failure.

Function API: `scope`, `bind`, `read`, `write`, `params`, and `WithParams`.
See the [macro example](examples/scoped_params.rs) and
[function example](examples/scoped_params_manual.rs).

## Actions and cancellation

`action(value)` adapts `BtAction` to `BtNode`:

```text
start → None         → Failure
start → Some(state)  → is_in_progress
    true             → tick → Running
    false            → complete → Success/Failure
```

Later updates query progress without restarting. State needs `Send + 'static`,
but not `Default`. Tick defaults to no work. All callbacks run inline and may
execute on a branch later rejected. Effects are not rolled back.

For external work, `start` submits a request, `is_in_progress` observes it, and
`complete` handles its outcome. The external system advances independently;
update the BT on completion or periodically with Evaluate for reactivity.

State owns cancellation. Use `CancelOnDrop::new(handle, cancel_fn)` or implement
`BtCancel` and use `CancelOnDrop::from(handle)`. Disarm in `complete` after handling
the outcome. Drop has no context argument; the handle must own cancellation access.
See the [external action example](examples/external_action.rs).

## Bevy

Enable the `bevy` feature. A tree's blackboard is an ordinary component the game
fills with its own systems; what the tree decides comes back out as components
the game matches on. A node never changes the world -- it starts an action and
waits for the world to finish it.

```toml
flatbt = { path = "../FlatBT", features = ["bevy"] }
```

```rust,ignore
use bevy::prelude::*;
use flatbt::bevy::prelude::*;

// What the tree may read, and what it decided.
#[derive(Component, Default)]
struct Guard {
    ammo: u32,
    alarm: bool,
    firing: bool,
    march_to: Option<f32>,
}

// What it decided, as components the game acts on.
#[derive(Component, Default, PartialEq)]
struct Firing;
#[derive(Component, PartialEq)]
struct MarchingTo(f32);

fn guard_tree() -> impl BehaviorNode<Guard> {
    select((
        seq((check(|g: &Guard| g.alarm && g.ammo > 0), action(Fire))),
        action(MarchTo),
    ))
}

app.add_plugins(BehaviorPlugin::for_tree(guard_tree))
    .add_plugins((
        ActionComponent::<_, Firing>::while_(|g: &Guard| g.firing),
        ActionComponent::describing(|g: &Guard| g.march_to.map(MarchingTo)),
    ))
    .add_systems(Update, gather.before(BehaviorSystems))
    .add_systems(Update, (shoot, march).after(ActionSystems));

commands.spawn((Ammo(2), Guard::default(), Behavior::for_tree(guard_tree)));
```

Nodes receive `&mut Guard`, a plain struct. No lifetimes appear in a tree, node
or action signature; `BtAction<Guard>` is written directly; node parameters
work, so `scope!` composes; and a tree is a value that takes a value, so it can
be exercised with no `World` at all.

| API | Behavior |
| --- | --- |
| `BehaviorPlugin::for_tree(builder)` | Builds one tree and adds its tick; `.in_schedule(..)`, `.parallel()`, `.tick_mode(..)` |
| `Behavior::for_tree(builder)` | Component holding one agent's invocation state |
| `Behavior::tick` | The tick itself, for a game that registers its own system |
| `BehaviorNode<C>` | What a tree over blackboard `C` is; also names a subtree |
| `ActionComponent` | Turns one standing decision into a component systems match on |
| `ActionSystems` | Set containing every such sync, for ordering game systems |
| `Tick` | What a tick does with one agent: `Evaluate`, `Resume` or `Skip` |
| `evaluate_every`, `act_every` | Periodic revalidation, staggered across agents |
| `BehaviorSystems` | Set containing every tick, for ordering game systems |

### Gather, decide, act

The blackboard is what an agent knows: an aggregate view of the world from its
own point of view, gathered before the tick. What it *decides* also starts as
fields of it, because a node sees `&mut C` and nothing else — but those fields
are a detail between the tree and its own registration, not an interface.
`ActionComponent` carries each of them out to a component, and that is what the
rest of the game reads.

Keeping both in one component is why the tick's query stays
`(&mut Behavior<C, F>, &mut C)` — disjoint per entity, which is why
`.parallel()` needs no further declaration. Using the blackboard to pass a value
between two nodes is fine; using it as the way the game learns what an agent is
doing is what `ActionComponent` exists to replace.

Filling it is the game's, deliberately. A real gather is several systems at
several rates: one for what is cheap enough every tick, another for a raycast,
another for a path query that only agents already in combat should pay for.
Ordering them is what Bevy is for:

```rust,ignore
app.add_systems(Update, (gather_cheap, gather_visibility).before(BehaviorSystems))
   .add_systems(Update, find_cover.before(BehaviorSystems).run_if(on_timer(..)))
   .add_systems(Update, carry_out.after(BehaviorSystems));
```

### Decisions come back out as components

A node sees `&mut C`, so what a tree decides starts life as a field. That is
fine between nodes and poor as an interface to the rest of the game: an ECS
matches on components, not on somebody's struct field. `ActionComponent` carries
each standing decision across, one line per decision:

```rust,ignore
app.add_plugins(BehaviorPlugin::for_tree(guard_tree).tick_mode(pace))
   .add_plugins((
       ActionComponent::describing(|g: &Guard| g.march_to.map(MarchingTo)),
       ActionComponent::<_, Reloading>::while_(|g: &Guard| g.reloading),
       ActionComponent::<_, Firing>::while_(|g: &Guard| g.firing),
   ));
```

After that the game is ordinary Bevy, and nothing outside the tree's own module
reads `Guard`:

```rust,ignore
fn refill(mut guards: Query<&mut Ammo, With<Reloading>>) { /* ... */ }
fn march(mut guards: Query<(&MarchingTo, &mut Post)>) { /* ... */ }
```

This is also what an action *is*. A node never changes the world: it starts
something, the component appears, a system does the work, and
`is_in_progress` watches the world until it is done. So `Reload` does not
subtract from the magazine and does not decide how long reloading takes — it
says "reloading" and waits for `refill` to say otherwise.

**What it costs.** Adding or removing a component moves the entity between
archetypes, so the price follows how often a decision *changes*, not how many
agents there are. Over 100 000 agents, whole frame, one decision left as a field
against the same decision as a component:

| an action lasts | as a field | as a component |
| --- | --- | --- |
| 1 tick | 0.57 ms | 8.06 ms |
| 10 ticks | 0.54 ms | 2.05 ms |
| 30 ticks | 0.51 ms | 1.28 ms |
| 120 ticks | 0.49 ms | 0.71 ms |

An instantaneous decision is ruinous as a component and fine as a field — and it
usually means the action wants writing as a `BtAction` that spans ticks rather
than a leaf that re-decides. In `games/arena`, five registered decisions over
100 000 fighters that change their minds constantly cost about 0.5-0.8 ms of
frame each; at a thousand agents, tens of microseconds.

### Asking the world

A tree reads and writes its blackboard and nothing else. When it needs something
the blackboard does not hold yet — a path, a line of sight, a nearest anything —
the only thing it can do is ask: write the question where the gather will see
it, and wait. `ask` is that as one node, and it belongs to the node catalog
rather than to Bevy, because nothing about it is ECS-specific.

```rust,ignore
scope! {
    let spot: Vec2;
    sequence {
        ask(|f: &mut Fighter| f.out().wants_cover = true, |f: &Fighter| f.cover)
            .with(out spot);
        WalkTo.with(spot);
    }
}
```

`WalkTo` then takes a `Vec2` rather than an `Option<Vec2>`, so it cannot run
without one. `ask` is an action rather than a leaf because a leaf returning
`Running` is re-entered on every resume, so a leaf that asks would ask again
every tick; `start` runs once per invocation, which is what asking means. And
the local belongs to the invocation, so leaving the branch and coming back asks
again instead of acting on an answer chosen for an older situation.

`Request<T>` — `Idle`, `Pending`, `Answered(T)` — keeps the question and the
answer in one field, and lets the system answering it match on `Pending` rather
than re-derive who wants an answer:

```rust,ignore
ask(|f: &mut Fighter| f.out().cover.ask(),
    |f: &Fighter| f.orders().cover.answered().copied())
```

The question has to be a field and cannot be a scope local, because a local
lives in the invocation state — a type no system can name. And a node cannot
run the query itself: a tree is built once into a resource and is `'static`,
while a `Query<'w, 's, ..>` borrows the world for one system run. Passing it
through the context is the design this one replaced; passing it through `params`
does not resolve (`&'a Query<'w, 's, ..>` is two levels of lifetime, and
`for<'a, 'w, 's>` over it gives "implementation of `BtNode` is not general
enough"); and a `&dyn` that would hide those lifetimes is blocked by `T: 'static
+ Sized` on `ParamValue`. The design note has the probes.

### Commands from a node

There are none, and `ActionComponent` is why: a decision that has to reach the
world reaches it as a component, which is the ECS's own way of saying an entity
is in a state. `Commands` in a node would also put lifetimes back into every
signature, since it borrows the world.

For the structural edits that are not a state — spawning a projectile, despawning
a corpse — the game does that in the system that matches the component, where it
has the `Commands` it needs anyway. A `CommandQueue` in the blackboard is the
unsupported alternative, kept as a worked example with its price in
`crates/flatbt-bevy/tests/commands.rs`: an unused queue costs 70% of the tick
over 200 000 agents, and one every agent writes to costs twenty times the frame.

### What lives where

A tree is an immutable definition, so it is built once into a resource. The
component holds only what is per-agent: the saved state of a suspended
invocation, sized exactly for that tree. Nothing is allocated, nothing is
reference counted, and dispatch stays static.

The builder function is the tree's name. It appears once at registration, where
it is called, and once per agent, where it only fixes the type; neither type
parameter is ever written out. Identity is the builder rather than the tree
type, so two builders may return the same tree type with different node
configuration and stay separate:

```rust,ignore
fn calm() -> impl BehaviorNode<Guard> { guard(1.0) }
fn angry() -> impl BehaviorNode<Guard> { guard(3.0) }
```

Spell the builder the same way at both sites. `shoot` and `shoot as fn() -> _`
are different names for the same tree; the second is writable, so
`Behavior<Guard, fn() -> _>` can appear in a query.

Only the builder's *type* selects the tree, and `Behavior` does not keep the
value: the tree was built once at registration. A closure that captures
configuration therefore configures nothing at the agent. Vary a tree with a
second builder function, not with captured values.

An agent whose tree was never registered ticks under no system and matches no
query — as with any component whose system is missing, nothing happens.

### Ticking

`BehaviorPlugin::for_tree(builder)` builds the tree and adds its tick when the
app is built, so the tick is in place before any agent exists and any schedule
will do: `First` through `Last`, `FixedUpdate`, or one the game runs itself. It
also carries that tree's ordering, run conditions and `.parallel()`.

Each tick reconsiders from the root by default. What a tick does with one agent
is the blackboard's answer, per agent per tick:

```rust,ignore
BehaviorPlugin::for_tree(guard_tree).tick_mode(|guard: &Guard| {
    if guard.marching {
        // A system outside the tree is carrying out what it decided.
        Tick::Skip
    } else if guard.alarm_changed {
        Tick::Evaluate
    } else {
        Tick::Resume
    }
})
```

`Evaluate` is the default because it is the answer that cannot be wrong: a tree
that only ever resumes never leaves the branch it is in, so `select` never
rescans and `choose!` never re-picks. A constant answer folds away. When a
resumed tick fails outright, the tick re-enters once with `Evaluate`, so a
standing decision that ran out never leaves an agent idle for a tick.

`Skip` does not enter the tree at all, and leaves the suspended invocation
exactly as it was. It is the one thing a tree cannot do for itself: a guard at
the root does not hold a suspended tree still, because `Resume` re-enters the
active child directly and `seq` continues its active child on `Evaluate` too;
even a `select`, which does rescan, runs its standing branch when the candidate
above it fails. Failing a candidate is how a tree *redirects*, not how it stops.

It is worth having when the work is elsewhere — an agent walking somewhere the
tree already chose has nothing to decide until it arrives. Over 200 000 agents
with nine in ten having nothing to decide:

| | serial tick |
| --- | --- |
| every agent enters the tree | 2.19-2.24 ms |
| nine in ten fail a guard at the root | 1.49-1.50 ms |
| nine in ten are `Skip`ped | 0.62-0.63 ms |

and the guard row is the optimistic one, since it only works at all for an agent
entering as `Evaluate` with nothing suspended below the guard.

For a tree that should simply rethink periodically, `evaluate_every` answers on
a period without putting the whole population on one frame. Each agent's slot
comes from its `Entity`, so nothing is stored and agents spawned together do not
share a due frame: over 64 agents at 16 ms ticks and a 200 ms period, the
busiest frame carries 6 of them rather than all 64. A frame longer than the
period evaluates once, never twice.

A tick's result is not reported anywhere. At the root it says only that this
invocation ended, and the next tick starts another; a tree that has something to
say to the game says it in the blackboard.

### Turn based, and stopping

Nothing here assumes a frame loop. Register the tick in whatever schedule the
turn runs in with `in_schedule`, and gate whose turn it is with `Tick::Skip`:

```rust,ignore
BehaviorPlugin::for_tree(fighter)
    .in_schedule(TurnPhase)
    .tick_mode(|agent: &Agent| if agent.has_turn { Tick::Resume } else { Tick::Skip })
```

A guard at the root would not do: a turn spanning several ticks leaves the tree
suspended, and a suspended tree does not consult a child above the one it is in.
`Skip` does not enter it at all, so the invocation waits exactly where it was and
resumes when the turn comes round — which is what FlatBT's saved state already
is, with nothing added for it.

Stopping every tree at once is a run condition on the set:

```rust,ignore
app.configure_sets(Update, BehaviorSystems.run_if(not(paused)));
```

`.parallel()` spreads agents across the task pool and accepts the same trees:
all agents of one tree tick concurrently. It needs `bevy_ecs`'s
`multi_threaded` feature, which the full `bevy` crate enables. Two *different*
trees overlap only if their blackboards differ, since Bevy schedules on declared
component access rather than on which entities match; trees over one blackboard
serialize, and either kind overlaps any unrelated system.

### Authoring

Trees are written with FlatBT's own API, with no Bevy-specific constructors:
`seq`, `select`, `check`, `leaf`, `choose!`, `scope!`, `action` and custom
`BtNode`s all take a plain blackboard as written. A subtree is a function
returning a node, so it composes into any tree by being called:

```rust,ignore
fn fire_at_intruder() -> impl BehaviorNode<Guard> { /* ... */ }

fn guard_tree() -> impl BehaviorNode<Guard> {
    select((seq((check(alarm_raised), fire_at_intruder())), leaf(patrol)))
}
```

A custom node names the blackboard directly, with no lifetimes to carry:

```rust,ignore
impl BtNode<Guard> for Reload { /* ... */ }
```

Tree state must be `Sync`, which Bevy requires of every component.

```sh
cargo run -p flatbt-bevy --example guards
```

## Custom nodes

Implement `BtNode<C, P = ()>`. Keep configuration in the definition and mutable
invocation data in `State: Default + Send + 'static`.
`P` carries parameters separately from context; the root supplies `()`.
Scopes bind references to local fields. `no_params(node)` adapts unit-parameter nodes.

A composing node stores descendant states and calls
`child.update(&mut state.child, ctx, params, mode)`. It owns initialization,
fresh-entry Evaluate, and cleanup on completion or replacement. Any node may
suspend without implementing `BtAction`; see [WaitFrames](examples/support/wait_frames.rs).

- Context mutations and ticks take effect immediately, including on failed branches.
- `NodeResult::error` and `ControlOp::error` log to stderr and return Failure.
  Ordinary Failure is silent.
- User panics propagate. Reset state before reuse after an unwind.
- Custom policies must terminate; there is no execution budget.
- Controls store one active child in an enum. Large states and nested candidates
  can require substantial stack space. State may move when a candidate is selected.

## Examples

Run any example with the default features:

```sh
cargo run --example resume
```

| Example | Shows |
| --- | --- |
| [synchronous](examples/synchronous.rs) | Composition and custom policy |
| [resume](examples/resume.rs) | Wait across updates, then continue |
| [revalidation](examples/revalidation.rs) | Preserve or preempt a saved branch |
| [choose](examples/choose.rs) | Context-driven and nested choice |
| [action](examples/action.rs) | Inline lifecycle |
| [external_action](examples/external_action.rs) | External work, 10 frames / 3 BT updates |
| [scoped_params](examples/scoped_params.rs) | Named local bindings |
| [scoped_params_manual](examples/scoped_params_manual.rs) | Equivalent function API |

## Advanced configuration

<details>
<summary>Selective features, direct crates, and tuple limits</summary>

### Select features

Core only:

```toml
flatbt = { path = "../FlatBT", default-features = false }
```

Core with selected helpers:

```toml
flatbt = { path = "../FlatBT", default-features = false, features = ["choose", "scope"] }
```

| Feature | API |
| --- | --- |
| `choose` | `choose!`, `ChooseNode`, `Choose` |
| `scope` | `flatbt::scope`: local storage, bindings, `scope!` |
| `action` | `BtAction`, `action`, cancellation helpers |
| `bevy` | `flatbt::bevy`: blackboard component, agent component, tick plugin |

Features are independent. Cargo combines features enabled by all consumers.
Core APIs are always available. Direct dependencies are also supported:

```toml
[dependencies]
flatbt-core = { path = "../FlatBT/crates/flatbt-core" }
flatbt-nodes = { path = "../FlatBT/crates/flatbt-nodes", features = ["choose", "action"] }
flatbt-scope = { path = "../FlatBT/crates/flatbt-scope" }
flatbt-bevy = { path = "../FlatBT/crates/flatbt-bevy" }
```

`flatbt-bevy` targets Bevy 0.19 and requires Rust 1.95.

### Tuple limits

Default: 32 children and 32 parameters. Configure in the consuming workspace's
`.cargo/config.toml`:

```toml
[env]
FLATBT_MAX_CHILDREN = "64"
FLATBT_MAX_PARAMS = "64"
```

Limits are non-negative build settings shared by all trees. Zero generates only
empty tuples. Changes regenerate on the next build. Existing environment variables
win; use `{ value = "64", force = true }` to override them.

Larger limits increase generated code and may require a higher macro recursion
limit. Tuples above the limit fail to compile. Custom `ParamShape` structs have
no generated tuple limit.

</details>

## API reference

From the checkout, run `cargo doc -p flatbt --open`.

[Contributing](CONTRIBUTING.md)
