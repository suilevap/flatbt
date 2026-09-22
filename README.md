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

let mut state: BtState<_, _> = BtState::new(&tree);
let mut ammo = 1;
assert_eq!(update(&tree, &mut state, &mut ammo, EntryMode::Resume), NodeResult::Success);
assert_eq!(ammo, 0);
```

One immutable tree can serve multiple agents. Each owns a `BtState` bound to that
tree. The tree must outlive its states. `update` rejects a different root.
A driver that cannot keep a `BtState` beside the tree it borrows, such as an ECS
component, holds an `Option<Tree::State>` itself and calls `update_slot`.

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
wants it can re-enter with `Evaluate` when a resumed update fails.
A completed child can be followed by another child in the same update.
During revalidation, a failed candidate preserves the old branch; a new Running
candidate replaces it and drops its state.

## What a tree decides

An update says whether the invocation ended, and if it did not, what the agent
is now *doing*. That is the payload of `Running`:

```rust
enum NodeResult<A = ()> { Success, Failure, Running(A) }
```

So an act cannot exist without something running, and nothing can run without
saying what it is doing — both by type, not by convention. A tree that finished
is not doing anything, and `Success` carries nothing.

```rust
#[derive(Debug, PartialEq)]
enum Act { MoveTo(f32), Reloading }

fn fighter() -> impl BtNode<Fighter, Act> {          // `Act` unifies; never written out
    select((
        seq((check(|f: &Fighter| f.ammo == 0), action(Reload))),
        action(Chase),
    ))
}

let doing = update(&tree, &mut state, &mut fighter, EntryMode::Evaluate).act();
assert_eq!(doing, Some(Act::MoveTo(120.0)));
```

An action says it in `tick`, which is asked on every update it is still in
progress — so a long action follows a moving target, restating where it is going
without ending:

```rust
impl BtAction<Fighter, Act> for Chase {
    type State = ();
    fn start(&self, _: &mut Fighter, _: ()) -> Option<()> { Some(()) }
    fn is_in_progress(&self, _: &(), f: &Fighter, _: ()) -> bool { !f.arrived() }
    fn tick(&self, _: &mut (), f: &mut Fighter, _: ()) -> Act { Act::MoveTo(f.player) }
}
```

Nodes that never occupy the agent never name the act type: `check` stays generic
over it, and so does any node that only succeeds or fails. The type therefore
unifies from the nodes that do decide, and appears in the tree's signature
without being declared anywhere else.

A tree whose nodes decide nothing uses the default act type, `()`. Say so where
the state is made — `BtState<_, _>` — and write `NodeResult::RUNNING` for a
running update that has nothing to report.

The act is what a driver applies to the world. Nothing in the tree touches it
after the update returns, so it is also what a driver can store, compare against
last update's, or hand to whatever carries decisions out.

Nothing constrains the act type either — it is only carried. An enum keeps the
whole vocabulary in one place and allocates nothing; a `Box<dyn Act>` whose
implementations apply themselves leaves the driver with no `match` at all, and a
new kind of order becomes a new type rather than an edit to an existing file, at
the price of an allocation per deciding update:

```rust,ignore
match update(&tree, &mut state, &mut guard, EntryMode::Evaluate).act() {
    Some(Act::WalkTo(to)) => guard.step_towards(to),   // enum
    ..
}

match update(&tree, &mut state, &mut guard, EntryMode::Evaluate).act() {
    Some(act) => act.apply(&mut guard),                // Box<dyn Act>
    None => {}
}
```

The [acts](examples/acts.rs) and [acts_dyn](examples/acts_dyn.rs) examples are
the same guard and the same tree in both forms, and print the same trace.

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
    true             → tick → Running(act)
    false            → complete → Success/Failure
```

Later updates query progress without restarting. State needs `Send + 'static`,
but not `Default`. `tick` both does inline work and returns the act — required
rather than defaulted, because an action is what occupies the agent and being
busy without saying with what is what this design removes. For a tree that
decides nothing the act type is `()` and the body is empty. All callbacks run
inline and may execute on a branch later rejected. Effects are not rolled back.

For external work, `start` submits a request, `is_in_progress` observes it, and
`complete` handles its outcome. The external system advances independently;
update the BT on completion or periodically with Evaluate for reactivity.

State owns cancellation. Use `CancelOnDrop::new(handle, cancel_fn)` or implement
`BtCancel` and use `CancelOnDrop::from(handle)`. Disarm in `complete` after handling
the outcome. Drop has no context argument; the handle must own cancellation access.
See the [external action example](examples/external_action.rs).

## Bevy

Enable the `bevy` feature. A tree reads a blackboard component and returns what
the agent is doing; the tick writes that into an act component, and ordinary
systems match it and do the work. A node never changes the world.

```toml
flatbt = { path = "../FlatBT", features = ["bevy"] }
```

```rust,ignore
use bevy::prelude::*;
use flatbt::bevy::prelude::*;

/// What the tree reads: an aggregate view of the world for this agent.
#[derive(Component, Default)]
struct Guard { ammo: u32, alarm: bool, reload_left: u32 }

/// What it decides. An order to the world, not a change to it.
#[derive(Component, Clone, Copy, PartialEq)]
enum Act { MarchingTo(f32), Firing, Reloading }

fn guard_tree() -> impl BehaviorNode<Guard, Act> {
    select((
        seq((check(alarm_raised), action(FireAt))),
        seq((check(|g: &Guard| g.ammo == 0), action(Reload))),
        action(MarchTo { where_to: |g: &Guard| g.intruder }),
    ))
}

app.add_plugins(BehaviorPlugin::for_tree(guard_tree))
    .add_systems(Update, gather.before(BehaviorSystems))
    .add_systems(Update, (march, shoot, refill).after(BehaviorSystems));

commands.spawn((Ammo(2), Guard::default(), Behavior::for_tree(guard_tree)));
```

One system then carries the act out, and never mentions the tree, the
blackboard, or FlatBT. Prefer a single exhaustive `match` over a system per
variant: adding an act stops it compiling until it is handled, where a system
per variant would silently ignore it.

```rust,ignore
fn carry_out(mut guards: Query<(&Act, &mut Ammo, &mut Destination)>) {
    for (act, mut ammo, mut destination) in guards.iter_mut() {
        let mut headed_for = None;
        match act {
            // Handed to whoever owns movement -- a path request, an animation
            // state, whatever that subsystem reads.
            Act::MarchingTo(target) => headed_for = Some(*target),
            Act::Firing => ammo.0 = ammo.0.saturating_sub(1),
            Act::Reloading => ammo.0 = (ammo.0 + 3).min(6),
        }
        destination.set_if_neq(Destination(headed_for));
    }
}
```

An arm need not do the work: delegating is often the point, and the act is a
good place to decide who gets it.

| API | Behavior |
| --- | --- |
| `BehaviorPlugin::for_tree(builder)` | Builds one tree and adds its tick; `.in_schedule(..)`, `.parallel()`, `.tick_mode(..)` |
| `Behavior::for_tree(builder)` | Component holding one agent's invocation state |
| `stop_behavior(builder)`, `restart_behavior(builder)` | Stop an agent or send it back to the root, on `Commands` or `EntityWorldMut` |
| `Behavior::tick` | The tick itself, for a game that registers its own system |
| `BehaviorNode<C, A>` | What a tree over blackboard `C` deciding `A` is; also names a subtree |
| `Tick` | What a tick does with one agent: `Evaluate`, `Resume` or `Skip` |
| `evaluate_every`, `act_every` | Periodic revalidation, staggered across agents |
| `BehaviorSystems` | Set containing every tick, for ordering game systems |

### The act is the interface

An agent doing something carries its act component; an agent whose tree ended
carries none. So `Query<&Act>` is exactly the agents with a standing order, and
`Query<&Name, (With<Guard>, Without<Act>)>` is exactly the idle ones — no flags,
and nothing to clear.

An act that only *changes* is written in place with `set_if_neq`, so an agent
that keeps doing the same kind of thing never moves archetype. Only appearing
and disappearing costs an insert or a remove, and those are batched. That is
what makes this affordable: a standing `MarchingTo(x)` that follows a moving
target is a value write per tick, not an archetype move.

This is also what an *action* is. A node never changes the world: it starts
something, the act appears, a system does the work, and `is_in_progress` watches
the world until it is done. `Reload` neither fills the magazine nor knows how
long that takes — it says `Reloading` and waits for `refill` to say otherwise.

The act belongs to the tick, which also takes it back. An agent is stopped by
removing its `Behavior`, and its standing order is released rather than left for
the world to go on obeying:

```rust,ignore
commands.entity(guard).stop_behavior(guard_tree);   // the act goes with it
commands.entity(guard).restart_behavior(guard_tree); // still an agent, back at the root
```

Stopping releases the act as the component goes — a removal hook, not a system —
so it does not wait for a tick that may never come, and it holds for a despawn
and for swapping one tree for another. Taking an agent's *blackboard* away is not
a way to stop it: nothing ticks it then, and it keeps the last order it was
given.

These take the builder because a `Behavior`'s type cannot be written down: a
builder's return type is opaque, so neither `remove::<Behavior<Guard, Act, _>>()`
nor a query over it can be spelled. Naming the tree by its builder is how
everything else here names it.

### The blackboard is input

Nodes get `&mut` to the blackboard — it is how they leave notes for each other —
but the tick passes it with change detection bypassed, so those writes are
invisible to `Changed<C>` and anything built on it. That is deliberate: a gather
rewrites the blackboard every tick anyway, and marking a whole population changed
every frame would drag the rest of the engine along. Anything the world should
notice is an act.

### Gather, decide, act

Filling the blackboard is the game's, deliberately. A real gather is several
systems at several rates: one for what is cheap enough every tick, another for a
raycast, another for a path query that only agents already in combat should pay
for. Ordering them is what Bevy is for:

```rust,ignore
app.add_systems(Update, (gather_cheap, gather_visibility).before(BehaviorSystems))
   .add_systems(Update, find_cover.before(BehaviorSystems).run_if(on_timer(..)))
   .add_systems(Update, carry_out.after(BehaviorSystems));
```

There is no way to issue an ECS command from a node: `Commands` borrows the
world and would put lifetimes back into every node signature. The act is the way
out, and a system that needs `Commands` has them where it matches the act.

### What lives where

A tree is an immutable definition, so it is built once into a resource. The
component holds only what is per-agent: the saved state of a suspended
invocation, sized exactly for that tree. Nothing is allocated, nothing is
reference counted, and dispatch stays static.

The builder function is the tree's name. It appears once at registration, where
it is called, and once per agent, where it only fixes the type; none of the three
type parameters is ever written out. Identity is the builder rather than the tree
type, so two builders may return the same tree type with different node
configuration and stay separate:

```rust,ignore
fn careful() -> impl BehaviorNode<Guard, Act> { armed(3) }
fn reckless() -> impl BehaviorNode<Guard, Act> { armed(1) }
```

Spell the builder the same way at both sites: `shoot` and `shoot as fn() -> _`
are different names for the same tree, and mixing them gives an agent whose tree
no tick matches. The type itself stays unwritable either way — a builder's return
type is opaque — which is why touching an agent's `Behavior` goes through
`stop_behavior` and `restart_behavior` rather than a query.

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

What a tick does with one agent is `tick_mode`'s answer, per agent per tick. It
receives the blackboard and a `TickAt`: the agent's `Entity` and the schedule's
`Time` (zero without one).

```rust,ignore
BehaviorPlugin::for_tree(guard_tree).tick_mode(|guard: &Guard, _| {
    if guard.alarm_changed { Tick::Evaluate }
    else if guard.walking { Tick::Skip }
    else { Tick::Resume }
})
```

`Evaluate` is the default because it is the answer that cannot be wrong. The
other two are optimisations that cost an agent responsiveness and never change
what it does once it runs — a tree that breaks under one of them has a bug.

`Skip` does not enter the tree at all and leaves both the suspended invocation
and the standing act alone, so the systems carrying that act out keep seeing it.
It is the one thing a guard inside the tree cannot do: once a tree is suspended,
no entry mode consults a child above the one it is in.

For a tree that should rethink periodically, `evaluate_every` answers on a period
without putting the whole population on one frame; each agent's slot comes from
its `Entity`, so nothing is stored. `act_every` is the same with `Skip` between
slots, for a tree whose every action is carried out by systems.

```rust,ignore
BehaviorPlugin::for_tree(guard_tree)
    .tick_mode(|_, at| evaluate_every(Duration::from_millis(250), at))
```

### Turn based, and stopping

Nothing here assumes a frame loop. Register the tick in whatever schedule the
turn runs in with `in_schedule`, and gate whose turn it is with `Tick::Skip`:

```rust,ignore
BehaviorPlugin::for_tree(fighter)
    .in_schedule(TurnPhase)
    .tick_mode(|agent: &Agent, _| if agent.has_turn { Tick::Resume } else { Tick::Skip })
```

A turn spanning several ticks needs no extra state: the invocation waits exactly
where it was, and so does the act. Stopping every tree at once is a run
condition on the set, which freezes both the same way:

```rust,ignore
app.configure_sets(Update, BehaviorSystems.run_if(not(paused)));
```

`.parallel()` spreads agents across the task pool and accepts the same trees. It
needs `bevy_ecs`'s `multi_threaded` feature, which the full `bevy` crate
enables. Two *different* trees overlap only if their blackboards and acts
differ, since Bevy schedules on declared component access rather than on which
entities match.

### Authoring

Trees are written with FlatBT's own API, with no Bevy-specific constructors:
`seq`, `select`, `check`, `leaf`, `choose!`, `scope!`, `action` and custom
`BtNode`s all take a plain blackboard and act as written. A subtree is a function
returning a node, so it composes into any tree by being called:

```rust,ignore
fn fire_at_intruder() -> impl BehaviorNode<Guard, Act> { /* ... */ }
```

A custom node names both directly, with no lifetimes to carry:

```rust,ignore
impl BtNode<Guard, Act> for Reload { /* ... */ }
```

Tree state must be `Sync`, which Bevy requires of every component.

```sh
cargo run -p flatbt-bevy --example guards
```

## Custom nodes

Implement `BtNode<C, A = (), P = ()>`. Keep configuration in the definition and
mutable invocation data in `State: Default + Send + 'static`.
`A` is what the node reports the agent is doing while it runs; a node that never
runs stays generic over it and never names it.
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
| [acts](examples/acts.rs) | What a tree decides, as an enum the driver matches |
| [acts_dyn](examples/acts_dyn.rs) | The same, with the act as a trait object |
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
| `bevy` | `flatbt::bevy`: blackboard and act components, tick plugin |

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
