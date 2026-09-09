# Local values, control flow, and node parameters

Status: implemented draft. No bt! compiler, FrameStorage, type-directed field
lookup, or runtime-owned heap allocation. This replaces the Scoped context and
double-reference parameter experiments.

## Module boundary

`flatbt::scope` contains scope storage, synchronous computation, bindings, and
the `scope!` macro. `use flatbt::scope::scope;` imports both the macro and the
function. Manual construction imports its helpers from the same module, for
example `use flatbt::scope::{bind, compute, read, write, scope};`.

Ordinary trees use the crate-root node and composition APIs without importing
scope. The shared `ParamShape`, `ParamValue`, `Read`, and `Write` types live in
`flatbt::params`; controls/actions depend on this protocol, not on scope storage
or bindings. `Read` and `Write` are also re-exported by `flatbt::scope` for tuple
bindings. This separation does not introduce conditional compilation or change
the node interface, state layout, or execution behavior.

## Independent contracts

Context, owned state, and parameters are separate:

```rust,ignore
trait BtNode<C, P = ()> {
    type State: Default + Send + 'static;
    fn update(&self, state: &mut Self::State, ctx: &mut C,
              params: P, mode: EntryMode) -> NodeResult;
}
```

LookAt requests `&Vector2` and receives `position: &Vector2`. A producer requests
`&mut Option<Vector2>` and receives exactly that reference. Multiple arguments
are an ordinary tuple such as `(&Enemy, &mut Option<Vector2>)`, passed by value.
Neither node knows the surrounding local container or field names. All nodes
keep the same World context. Passing references by value does not transfer
ownership of the local data, and State cannot retain update-local references.

`BtAction<C, P = ()>` receives P directly in every callback, including the progress
query. Read inputs stay shared; declared outputs remain exclusive. Parameters
are borrowed anew each update. An action requiring a snapshot must copy an owned
value or ID into its own state, including for external work and cancellation.

## Synchronous locals and explicit control flow

```rust,ignore
use flatbt::scope::scope;

scope! {
    context: World;
    let walk_pos: Vector2 = |bb| bb.next_patrol_pos;
    let door_pos: Vector2 = get_visible_door_pos;
    sequence {
        LookAt.with(door_pos);
        wait_frames(1);
        action(Walk).with(walk_pos);
    }
}
```

A local initializer is `Fn(&mut World) -> T`, not a node. The optional context
header supplies its argument type, permitting short `|bb| bb.field` closures.
Without the header, use `|bb: &mut World| bb.field`. Named functions have the same
mutable-context signature. Captured configuration belongs to the definition;
constructing the tree does not call the initializer.

Initializers execute once per scope invocation, in declaration order, before any
body child. Their values survive Running, Resume, and Evaluate. The next entry
after completion/reset initializes fresh values. Initializers currently receive
context only, not other local fields, and return a value without a BT failure or
Running result. A returned Option/Result is itself the local value, not implicit
control flow. Use a producer node when selection needs a BT result or suspension.

There is no default body control. The macro requires sequence or select:

```rust,ignore
scope! {
    context: World;
    let primary: Vector2 = |bb| bb.next_patrol_pos;
    let fallback: Vector2 = |bb| bb.visible_door_pos;
    select {
        sequence {
            check(|bb: &World| bb.patrol_allowed);
            action(Walk).with(primary);
        }
        action(Walk).with(fallback);
    }
}
```

The selector revalidates its candidates on Evaluate without rerunning the local
initializers. Resume follows its saved branch. Both initializers above run before
selection; for branch-lazy computation put a separate scope inside that branch.
Nested sequence/select blocks share their containing scope's locals. A nested
scope owns independent locals and does not implicitly inherit its parent's slots.

## Calls and optional output slots

`LookAt.with(door_pos);` explicitly supplies an input. Plain expressions such as
`Wait;`, `wait_frames(1);`, and `leaf(|bb: &mut World| { ... });` use unit parameters.
Configured definitions, paths, and adapters work directly:
`action(Walk).with(walk_pos);`. No `run` keyword or square brackets are required.

Constructor arguments are always ordinary Rust expressions. Only the final
`.with(...)` suffix binds runtime local names; the macro never guesses whether a
constructor argument names a local. Locals are not definition-time Rust variables.
For example, `wait_frames(frames)` uses an outer Rust configuration variable,
while `LookAt.with(frames)` selects the scope field, even if both names exist.
To use an unrelated existing `.with` method, parenthesize that complete expression.
Binding syntax is recognized at the statement level, not inside opaque Rust
expressions such as tuple arguments; use nested sequence/select blocks for locals.

Only producers that need a node protocol require explicit output slots:

```rust,ignore
scope! {
    context: World;
    let enemy: Enemy = |bb| bb.best_enemy;
    let cover_pos: Vector2;
    sequence {
        ChooseCover.with(enemy, out cover_pos);
        Move.with(cover_pos);
        Wait;
        Attack.with(enemy);
    }
}
```

ChooseCover requests `(&Enemy, &mut Option<Vector2>)`; it can suspend before
filling the output. Its own state ends when it completes, but the scope retains
the output for later siblings. A successful producer that leaves None does not
silently supply a value: the consuming binding reports a diagnostic and fails.
The optional `in name` spelling also denotes a shared input. Multiple arguments
form a tuple in written order, independently of their types.

## Function API and implementation

`scope::<Locals, _>(subtree)` itself has no control policy and initializes Locals
with Default. The macro generates Option fields, initializes them with bound
`compute(callback)` nodes, and places the requested control after that prefix in
an ordinary sequence. The sequence preserves the active body on Evaluate, so
initializers are not replayed. With no initializers there is no prefix wrapper.

The function API supports arbitrary controls and binding projections:

```rust,ignore
use flatbt::scope::{bind, params, read, scope, Read, Write};

scope::<Locals, _>(select((
    bind(LookAt, read(|s: &Locals| s.door_pos.as_ref())),
    bind(action(Walk), read(|s: &Locals| s.walk_pos.as_ref())),
)))
bind(ChooseCover, params::<(Read<Enemy>, Write<Option<Vector2>>), _, _>(
    |s: &mut CombatLocals| Some((s.enemy.as_ref()?, &mut s.cover_pos)),
))
```

With the `WithParams` extension trait imported, `node.with(binding)` is the same
operation as `bind(node, binding)` in ordinary Rust. In scope!, `.with(local)`
generates the binding projection, so both authoring paths use the same operation.

Read/write projections select concrete fields; Rust verifies disjoint writes.
Repeated shared inputs are valid, but two exclusive outputs cannot alias, nor
can one call read and write the same slot. Unknown/duplicate fields and wrong
parameter types are compile errors. Missing values are detected at runtime.

ParamShape describes a lifetime-indexed view and how to reborrow it. ParamValue
maps concrete unit/reference/tuple values to that shape. Controls and the action
adapter use these traits internally to lend the same parameters across successive
calls. Ordinary leaf/action authors use plain references and need no explicit
reborrowing. Custom parameter structs used by controls/actions need corresponding
ParamShape/ParamValue implementations; ParamBinding describes custom projections.

Tuple implementations are generated through FLATBT_MAX_PARAMS (default 32),
independently of FLATBT_MAX_CHILDREN. Both can be configured at build time; no
runtime code generation or type erasure is involved. The initializer prefix and
each body control separately obey the child-count limit. Large macros can also
require a higher Rust recursion limit.

## State and effects

ScopeState stores child state before Locals so descendants drop first. Macro
slots are Option<T>; payloads need Send + static, but not Default or Clone. The
child subtree retains the normal active-child enum. All storage stays inline.

Completion, rejection, preemption, reset, and BtState destruction release owned
locals. Independent BtState instances own independent values. Branch-private
scopes isolate their values. Writes to an enclosing scope shared by selector
candidates remain visible after candidate failure, like context effects; there
is no transactional rollback. Params are live views, not implicit snapshots.

Root update still supplies unit parameters without changing its external call.
Custom composing nodes now pass parameters by value or explicitly reborrow them
for successive calls. Control policies still observe context only; choose! does
not automatically read locals. no_params adapts unit-parameter nodes to any
surrounding parameter contract. The macro applies it to plain node expressions.

Tests cover initialized values across suspension and selector revalidation,
construction versus entry timing, initialization order and fresh entry, mixed
parameter reborrowing across action callbacks, named same-type bindings, producer
suspension, independent instances, missing inputs, shared writes, and scope cleanup.
Compile-fail doctests check shared access and exclusive aliasing. The runnable
example is `cargo run --offline --example scoped_params`.

`examples/scoped_params_manual.rs` is the equivalent tree written using functions
only, with the same initialization prefix, bindings, and execution assertions.
