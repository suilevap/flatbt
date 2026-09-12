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

Enable the `bevy` feature. Declare the world access one family of trees needs,
add one plugin, then give agents a behavior naming the tree they run. Trees
build and register themselves when their first agent appears.

```toml
flatbt = { path = "../FlatBT", features = ["bevy"] }
```

```rust,ignore
use bevy::prelude::*;
use bevy::ecs::query::QueryData;
use flatbt::bevy::prelude::*;

#[derive(QueryData)]
#[query_data(mutable)]
struct Guard {
    ammo: &'static mut Ammo,
    post: &'static mut Post,
}

impl BehaviorContext for Guard {
    type Agent = Self;                  // the agent's own components
    type Param = Res<'static, Alarm>;   // shared, read-only
}

fn guard_tree() -> impl BehaviorNode<Guard> {
    select((
        seq((
            check(|bt: &Bt<Guard>| bt.shared.raised && bt.ammo.0 > 0),
            leaf(|bt: &mut Bt<Guard>| {
                bt.ammo.0 -= 1;
                NodeResult::Success
            }),
        )),
        leaf(patrol),
    ))
}

app.add_plugins(FlatBtPlugin::new());
commands.spawn((Ammo(2), Post(0.0), Behavior::for_tree(guard_tree)));
```

Nodes receive `Bt<C>`, which derefs to the agent view: `bt.ammo` reaches the
agent's component, `bt.shared` the read-only world access, `bt.entity` and
`bt.commands` everything else.

| API | Behavior |
| --- | --- |
| `BehaviorContext` | Declares `Agent` (per-entity components), `Param` (shared, read-only) and `entry_mode` |
| `FlatBtPlugin::new()` | Added once; trees register themselves from their first agent |
| `BehaviorPlugin::for_tree(builder)` | Registers one tree ahead of time; `.in_schedule(..)`, `.parallel()` |
| `Behavior::for_tree(builder)` | Component holding one agent's invocation state |
| `evaluate_every` | Periodic `entry_mode` answer, staggered across agents |
| `BehaviorSystems` | Set containing every tick, for ordering game systems |

### What lives where

A tree is an immutable definition, so it is built once into a resource. The
component holds only what is per-agent: the saved state of a suspended
invocation, sized exactly for that tree. Nothing is allocated, nothing is
reference counted, and dispatch stays static.

The builder function is the tree's name. It appears once at registration, where
it is called, and once per agent, where it only fixes the type; neither type
parameter is ever written out, and `impl BehaviorNode<C>` names a subtree the
same way. Identity is the builder rather than the tree type, so two builders may
return the same tree type with different node configuration and stay separate:

```rust,ignore
fn calm() -> impl BehaviorNode<Guard> { guard(1.0) }
fn angry() -> impl BehaviorNode<Guard> { guard(3.0) }
```

Spell the builder the same way at both sites. `shoot` and `shoot as fn() -> _`
are different names for the same tree; the second is writable, so
`Behavior<Guard, fn() -> _>` can appear in a query. A mismatch, or a missing
registration, is reported when the component is added rather than left as an
agent that never ticks.

### Registration

`FlatBtPlugin` is the only required line. The first agent naming a tree builds it
and registers its tick, so adding a tree is writing a builder and spawning an
agent. An agent spawned before the tick schedule runs — in `Startup`, say — ticks
that same frame; one spawned from inside the tick schedule starts on the next,
because a schedule cannot be extended while it runs. Later agents of a registered
tree tick immediately.

`BehaviorPlugin::for_tree(builder)` registers a tree ahead of its agents, for one
that needs its own schedule, ordering, run conditions, or `.parallel()`. An
explicitly registered tree is left alone by self-registration.

An agent whose tree was never built ticks under no system and matches no query,
so nothing else could report it. It is reported by name when the component is
added, which also covers a builder spelled one way at registration and another
at the agent.

### Ticking

A tick resumes: a suspended invocation continues down the path it chose, and a
finished one starts fresh, which always enters as `Evaluate`. Reconsidering a
standing decision costs work, so when it happens is the context's call, decided
per agent per tick from the world it already declared:

```rust,ignore
impl BehaviorContext for Guard {
    type Agent = Self;
    type Param = Res<'static, Alarm>;

    fn entry_mode(bt: &Bt<Guard>) -> EntryMode {
        if bt.shared.is_changed() { EntryMode::Evaluate } else { EntryMode::Resume }
    }
}
```

It defaults to `Resume`, and a constant answer folds away. On `Evaluate` a
`select` rescans its children from the first, while a `seq` continues its active
child, so revalidation reconsiders choices rather than restarting work.

For a tree that should simply rethink periodically, `evaluate_every` answers on
a period without putting the whole population on one frame:

```rust,ignore
fn entry_mode(bt: &Bt<Guard>) -> EntryMode {
    evaluate_every(
        Duration::from_millis(200),
        bt.shared.elapsed(),
        bt.shared.delta(),
        bt.entity,
    )
}
```

Each agent's slot within the period comes from its `Entity`, so nothing is
stored and agents spawned together do not share a due frame: over 64 agents at
16 ms ticks, the busiest frame carries 6 of them rather than all 64. A frame
longer than the period evaluates once, never twice.

A tick's result is not reported anywhere. At the root it says only that this
invocation ended, and the next tick starts another; a tree that has something to
say to the game says it through `bt`, as a component or a command.

### Turn based

Nothing here assumes a frame loop. The agent query is the gate, so a marker the
game moves ticks exactly one agent:

```rust,ignore
#[derive(QueryData)]
#[query_data(mutable)]
struct Fighter {
    _turn: &'static Turn,            // only the holder is ticked
    journal: &'static mut Journal,
}
```

The game owns the order by moving `Turn`, and the tree hands it on when its turn
ends (`bt.agent_commands().remove::<Turn>()`). Register the tick in whatever
schedule the turn runs in, with `in_schedule`, or gate it with a run condition.

A turn spanning several ticks needs no extra state: the agent's invocation stays
suspended while it is not its turn and resumes exactly where it left off, which
is what FlatBT's saved state already is. Because the gate is a query, agents out
of turn cost nothing at all — they are not even iterated.

Stopping agents needs nothing from the crate. A run condition on
`BehaviorSystems` halts every tree, including ones whose tick system was added
by self-registration afterwards:

```rust,ignore
app.configure_sets(Update, BehaviorSystems.run_if(not(paused)));
```

Halting one agent, or one tree, is what a behavior tree is already for: put the
condition at the root and let the tree fail fast.

Agent access must be disjoint per entity, and shared access is read-only, so
`.parallel()` spreads agents across the task pool with no further declaration
and accepts the same trees: all agents of one tree tick concurrently. It needs
`bevy_ecs`'s `multi_threaded` feature, which the full `bevy` crate enables;
without it `.parallel()` falls back to iterating in order. Everything beyond the
agent's own components is deferred through `bt.commands`.

Two *different* trees over one context overlap only if their declared access
allows it: Bevy schedules on declared component access, not on which entities
match. Two trees whose `Agent` writes `&mut Health` serialize, even though no
agent runs both. Make `Agent` read-only and route changes through `bt.commands`
and they run concurrently — at the price of writes landing at the next sync
point rather than immediately, so it is worth it for trees that mostly observe,
not for hot per-agent state. Trees over different contexts always overlap, as
does a tick with any unrelated system.

### Authoring

Trees are written with FlatBT's own API, with no Bevy-specific constructors:
`seq`, `select`, `check`, `leaf`, `choose!`, `scope!`, `action` and custom
`BtNode`s all take a Bevy context as written. A subtree is a function returning
a node, so it composes into any tree by being called:

```rust,ignore
fn fire_at_intruder() -> impl BehaviorNode<Guard> { /* ... */ }

fn guard_tree() -> impl BehaviorNode<Guard> {
    select((seq((check(alarm_raised), fire_at_intruder())), leaf(patrol)))
}
```

Only the root is named by a builder, and only because the component type is
derived from it. FlatBT's constructors check their
callable where the tree runs rather than where it is built, which is what keeps
an inline closure usable at any update.

A custom node names the context in full: its lifetimes are Bevy's, from
`QueryData::Item<'w, 's>` and `Commands<'w, 's>`, and cannot be collapsed.

```rust,ignore
impl BtNode<Bt<'_, '_, '_, '_, '_, Guard>> for Reload { /* ... */ }
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
| `bevy` | `flatbt::bevy`: Bevy ECS context, agent component, tick plugin |

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
