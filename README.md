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

Features are independent. Cargo combines features enabled by all consumers.
Core APIs are always available. Direct dependencies are also supported:

```toml
[dependencies]
flatbt-core = { path = "../FlatBT/crates/flatbt-core" }
flatbt-nodes = { path = "../FlatBT/crates/flatbt-nodes", features = ["choose", "action"] }
flatbt-scope = { path = "../FlatBT/crates/flatbt-scope" }
```

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
