# FlatBT

An experimental Behavior Tree runtime in Rust. Development proceeds in small,
working iterations, using code-authored definitions and compiler-known state
layouts as the foundation.

The current implementation supports synchronous composition, suspension, normal resume,
root re-evaluation, preemption, context-driven choice among statically known nodes,
invocation-local values shared by a subtree, and a draft action lifecycle. It is based on the simple M1 implementation;
see the [draft design notes](docs/design/static-state-draft.md) for state composition.

```rust
use flatbt::{BtState, EntryMode, NodeResult, check, leaf, seq, update};

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

`BtState::new(&root)` creates state bound by reference to the root definition.
The free `update` function takes the root, state, context, and entry mode. It checks
the root binding before executing. Completion and `reset()` discard saved state
while preserving the binding. One root can serve multiple independent instances
and must outlive them. The node trait has no state-construction method.

`EntryMode::Resume` follows the saved path. `EntryMode::Evaluate` revalidates from
the root: Sequence preserves its active child, while Selector scans from child
zero. Fresh invocations always receive Evaluate, regardless of the requested mode.
A failed candidate leaves the old branch state intact; a new Running candidate
preempts it. A terminal result releases the invocation, so the next update starts fresh.

Each node's `State` includes the state of its statically known descendants.
`ControlNode::State` combines policy state with a generated enum containing Empty
or one active child's state. The variant also encodes the active child index.
Tuple dispatch borrows an existing payload directly. During revalidation, a new
candidate runs in a local variable while the old payload remains intact. Terminal
candidates are dropped; a Running candidate replaces and drops the old payload.

Persistent child state therefore reserves space for the largest alternative plus
the enum tag and alignment, rather than space for all alternatives together.
Candidate evaluation needs temporary call-stack space and moves selected state
into the enum. Ordinary resume updates the saved payload in place. Controls with
multiple simultaneously active children would need a different state layout.

The runtime has no frame stack, scratch storage backend, or type erasure. Static
state layout is known to Rust, and the runtime adds no heap allocations. A custom
node can still own allocating resources in its state. Frame storage and layout
descriptors remain deferred to open sets of runtime-defined node types.

`choose!` selects among compiler-known node types using a match on shared context:

```rust
let tree = choose!(|bb: &Blackboard| match bb.order {
    Order::Move => MoveNode,
    Order::Attack => AttackNode,
    Order::Idle => IdleNode,
});
```

All arm definitions are constructed once when the tree is built. Evaluate repeats
the match; Resume follows the saved arm. The selected child's result is returned
directly, without fallback to another arm. State uses the existing inline child
enum, including for nested `choose!` calls. No manual enum, indices, type erasure,
or runtime heap allocation is needed. See the [choice draft](docs/design/choose-draft.md)
for construction semantics and the initial syntax limits.

The separate `flatbt::scope` module provides local storage and bindings. Ordinary
trees need no imports from it. `scope!` owns named local values in the tree state,
computes them once on entry, and explicitly selects control flow. The application
context stays unchanged:

```rust,ignore
use flatbt::scope::scope;

let tree = scope! {
    context: World;
    let walk_pos: Vector2 = |bb| bb.next_patrol_pos;
    let door_pos: Vector2 = get_visible_door_pos;
    sequence {
        LookAt.with(door_pos);
        wait_frames(1);
        action(Walk).with(walk_pos);
    }
};
```

Initializers are ordinary `Fn(&mut World) -> Vector2` functions/closures, not
nodes. `context: World;` lets closures omit parameter types; otherwise annotate
them individually. Use `select { ... }` for fallback/priority selection. Controls
can nest and share the same locals. Initializers run before the selected control,
in declaration order, and are not replayed on Resume or Evaluate while Running.

`LookAt` implements `BtNode<World, &Vector2>` and receives `position: &Vector2`.
`Walk` implements `BtAction<World, &Vector2>`; every callback receives a single
reference. Neither node knows field names or the scope layout. Named arguments
select their fields explicitly, even when several fields share one type.

A suspending producer can still fill a declared slot: `let cover_pos: Vector2;`
followed inside the body by `ChooseCover.with(enemy, out cover_pos);`. It requests
`(&Enemy, &mut Option<Vector2>)`, passed directly as a tuple of references.
Synchronous initializers need no user-authored output parameter. Plain expressions
such as `Wait;`, `wait_frames(1);`, and `leaf(|bb: &mut World| { ... });` use unit
parameters. Constructors keep ordinary Rust arguments; `.with(...)` explicitly
binds runtime locals. There is no `run` keyword or square-bracket wrapper.

The macro emits a local struct, `scope::<Locals, _>`, an initialization prefix,
the selected controls, and `bind` adapters. The function API also supports custom
controls and projections: `bind(LookAt, read(|s: &Locals| s.door_pos.as_ref()))`.
The function API lives in `flatbt::scope` as well; import helpers such as
`use flatbt::scope::{bind, read, scope, WithParams};`.
Import `WithParams` to use the equivalent fluent spelling outside the macro:
`LookAt.with(read(|s: &Locals| s.door_pos.as_ref()))`.

Locals drop when the scope ends. Missing inputs report a diagnostic and fail the
bound node; writes to shared enclosing locals survive failed candidates. See the
[local state draft](docs/design/local-state-draft.md) for contracts and limitations
and the runnable [parameter binding example](examples/scoped_params.rs).

Examples:

```sh
cargo run --offline --example synchronous
cargo run --offline --example resume
cargo run --offline --example revalidation
cargo run --offline --example action
cargo run --offline --example external_action
cargo run --offline --example choose
cargo run --offline --example scoped_params
cargo run --offline --example scoped_params_manual
```

The resume example uses an application-defined `WaitFrames` from
`examples/support/wait_frames.rs`, shared with tests. It suspends for three updates
and executes the next child on completion. The revalidation example preserves a
patrol while a higher-priority candidate fails, then preempts it when that candidate
becomes eligible.

`action(value)` adapts `BtAction` to ordinary `BtNode::update`. The lifecycle is
`start → is_in_progress → tick` while Running, followed by `complete` as soon as
a later progress query returns false. Start returning None fails immediately.
Action state does not need Default; the adapter stores Option<A::State>.

Tick runs inline in this draft, including during speculative traversal. Its
effects survive rejection by a parent. Completing one action still lets Sequence
advance and tick the next action in the same update. The action example shows
this behavior. See the [action draft](docs/design/action-draft.md) and the
[archived post-commit experiment](experiments/README.md) for the tradeoff.

For externally scheduled work, start submits an operation and returns its request
handle, is_in_progress observes it, and dropping the cancellation guard requests
cancellation. Tick has an empty default and need not be implemented. The scheduler
advances work without running the BT; the application updates the BT on completion
events or at a lower frequency with Evaluate for reactivity. The external_action example performs ten
external frames with only three BT updates. It uses an application-owned movement
component and no async runtime or engine dependency.

Cancellation is owned by action state: a cancel-on-drop handle can stop external
work when the state is preempted, rejected, reset, or dropped. `complete` receives
`&mut State` so it can disarm the handle after normal completion. The tree has no
cancel traversal. Actions that need no cancellation carry no cancellation
metadata. The external example uses a request-specific
atomic token, with one allocation on external submission and none on Resume.
State must own its cancellation access; Drop has no BB argument.

To keep acquisition and cancellation together, start can return
`CancelOnDrop::new(request, |request| { /* request cancellation */ })`. This
accepts a function or a non-capturing closure; cancellation data belongs in the
request. The external example uses this form and needs no separate trait impl.
Call `state.disarm()` in complete after handling success or failure.

For handles with reusable cancellation logic, implement
`BtCancel::cancel(&mut self)` and wrap them with `CancelOnDrop::from(handle)`.
Both forms provide typed access through Deref/DerefMut and store the value plus
an optional function pointer inline. There is no allocation or separate armed
flag; cancellation may use an indirect function call. Existing custom Drop
implementations can still be used directly as action state, without this wrapper.

The crate root provides `BtNode`, `BtControl`, and composition primitives: `seq`,
`select`, `check`, `leaf`, and `choose!`. Local storage, `scope!`, and bindings live
in `flatbt::scope`. The `flatbt::params` module provides the shared parameter
traits used by controls/actions, independently of scopes. These are module
boundaries, not Cargo feature flags. Tuple children of arity 0–32 use static
dispatch by default. A reusable catalog of utility nodes and policies belongs in a separate crate if introduced
later. Example helpers are not core exports.

To change the maximum tuple arity, put this in the consuming project's
`.cargo/config.toml` (at the workspace root when using a Cargo workspace):

```toml
[env]
FLATBT_MAX_CHILDREN = "64"
FLATBT_MAX_PARAMS = "64"
```

Cargo passes this setting to FlatBT's build script, including when FlatBT is a
dependency. The default is 32; zero generates only the empty-tuple implementation.
The value must be a non-negative integer. Changing it triggers regeneration on
the next build without editing FlatBT or running `cargo clean`. An existing
environment variable takes precedence over this config entry; use Cargo's
`{ value = "64", force = true }` form if the config must override it.

This is a build setting shared by consumers of that FlatBT build, not a runtime
or per-tree setting. Larger values generate more Rust code and increase build
cost; very large values can exceed the compiler's macro recursion limit. A tuple
above the configured limit is rejected at compile time.

`FLATBT_MAX_PARAMS` independently sets the maximum generated parameter tuple arity
for `params` and `scope!` bindings (default 32). It uses the same configuration and
regeneration rules. It does not limit custom `ParamShape` structs. Larger arities
increase generated code and may require a higher Rust macro recursion limit.

Validation:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
```

Custom nodes use `State: Default + Send + 'static`, separate from their definition.
`BtNode<C, P = ()>` accepts parameters separately from context and node state.
The root API supplies `()`; a scope supplies its owned local fields to bound
children. Parameter-free custom nodes accept a `()` parameter; use
`no_params(node)` to reuse them under any scope. `BtAction<C, P = ()>` similarly
receives parameter values directly in each callback.
A composing node includes nested state fields and calls a child with the chosen
field: `child.update(&mut state.child, ctx, params, mode)`. It owns initialization and
cleanup when nested invocations start, finish, or are replaced.
A node can suspend without implementing BtAction. Empty sequences succeed; empty
selectors fail. Ordinary Failure is silent; `NodeResult::error` and
`ControlOp::error` report execution errors to stderr and return Failure.

Context changes, including action ticks, take effect immediately and survive
failed branches. There is no post-commit phase in this draft. User-code panics
are not caught; after an unwind,
reset the state before using it again. Custom policies must ensure termination;
there is no execution budget.

The API is experimental. Open sets of runtime-defined node types, their storage,
and the `bt!` compiler remain future work. Memory requirements for open candidate
sets will be considered separately from the current static composition model.

The [decision log](docs/design/decisions.md) records earlier iterations.
The [original architecture document](docs/design/original-architecture.md) is an
unmodified Russian source snapshot for discussion, not a binding contract.
