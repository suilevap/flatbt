# Local values and node parameters

Implemented draft. Replaces the Scoped-context and double-reference experiments.
Inline state; no bt! compiler, FrameStorage, type-based field lookup, or runtime
heap allocation.

## API boundary

Module `flatbt::scope` holds storage, computation, bindings, and `scope!`.
`use flatbt::scope::scope;` imports both macro and function.

Core's `flatbt::params` owns `ParamShape`, `ParamValue`, `Read`, and `Write`.
Controls/actions use this protocol independently of scope storage. Scope also
re-exports Read/Write for bindings.

## Parameters

```rust,ignore
trait BtNode<C, P = ()> {
    type State: Default + Send + 'static;
    fn update(&self, state: &mut Self::State, ctx: &mut C,
              params: P, mode: EntryMode) -> NodeResult;
}
```

| Request | Received value |
| --- | --- |
| Shared input | `&Vector2` |
| Output slot | `&mut Option<Vector2>` |
| Multiple arguments | `(&Enemy, &mut Option<Vector2>)`, in written order |

Nodes keep the same context type and know neither field names nor scope layout.
Parameters borrow locals for each update; state cannot retain those references.
`BtAction<C, P>` receives P in every callback, including progress queries. Store
an owned value or ID in action state when a snapshot or cancellation access is needed.

## Initializers and controls

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

- Initializers are `Fn(&mut World) -> T`. The optional context header supplies the
  argument type; otherwise annotate closure arguments.
- Tree construction stores definitions/captures. Initializers run once on entry,
  in declaration order, before any body child.
- Values survive Running, Resume, and Evaluate. Completion/reset permits fresh
  initialization on the next entry.
- Initializers receive context only. Option/Result returns are values, not BT
  control flow. Use a producer node for failure or suspension.
- Body control is explicit: `sequence` or `select`. Nested controls share locals.
- Nested scopes own independent locals and inherit no slots. Put a scope inside
  a branch for lazy initialization; enclosing initializers run before selection.

A selector body rescans on Evaluate without replaying initializers:

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

## Calls and output slots

`Node.with(name)` or `Node.with(in name)` binds a shared input; `out name` binds
an exclusive output. Plain node expressions use unit parameters. Constructors
keep ordinary Rust arguments: `wait_frames(frames)` reads outer configuration;
`LookAt.with(frames)` selects a local field.

A leading `with(...)` binds the whole node after it, up to `;`, with the same
argument syntax; use it when a long node would push the suffix out of sight.
Otherwise only the final statement-level `.with(...)` suffix binds locals. Parenthesize an
expression to use an unrelated `.with` method. Binding syntax inside opaque Rust
expressions, such as tuple arguments, is not expanded; use nested control blocks.

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

ChooseCover receives `(&Enemy, &mut Option<Vector2>)` and may suspend before writing.
The output survives producer completion. If the producer leaves None, the consumer
logs a diagnostic and fails without running.

Rust rejects unknown/duplicate fields, wrong parameter types, overlapping exclusive
outputs, or reading and writing the same slot in one call. Repeated shared inputs
are valid. Missing values are runtime errors.

## Function API

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

`scope::<Locals, _>(subtree)` initializes Locals with Default and adds no control
policy. Import `WithParams` for `node.with(binding)`, equivalent to `bind(node, binding)`.
The function API accepts custom controls and projections.

The macro generates Option fields, bound `compute(callback)` initializers, and an
outer sequence containing initialization plus the requested body. Sequence preserves
the active body on Evaluate. No initializers means no prefix wrapper.

## Reborrowing and limits

ParamShape defines a lifetime-indexed view and reborrowing; ParamValue maps concrete
values to it. Controls/actions reborrow between calls. Ordinary nodes use references
directly. Custom parameter structs used by controls/actions need both traits;
custom projections implement ParamBinding.

`FLATBT_MAX_PARAMS` controls tuple generation (default 32), independently of
`FLATBT_MAX_CHILDREN`. The initialization prefix and each control obey the child
limit separately. Large macros may need a higher Rust recursion limit.

## State and effects

ScopeState stores child state before locals, so descendants drop first. Macro slots
are inline `Option<T>`; payloads need `Send + 'static`, not Default or Clone.
Each BtState owns independent locals. Completion, rejection, preemption, reset,
and Drop release them.

Writes to an enclosing scope survive failed candidates, like context writes.
Branch-private scopes isolate their own values. Parameters are live views.
Policies still read context only; choose! does not automatically read locals.
Root update supplies `()`. The macro uses `no_params` for plain nodes; custom
composers pass or reborrow parameters explicitly.

## Validation

Tests cover entry timing/order, suspension, revalidation, fresh values, callback
reborrowing, same-type fields, producers, independent instances, missing inputs,
shared writes, and cleanup. Compile-fail doctests cover shared access and aliasing.

Run `cargo run --offline --example scoped_params`.
[scoped_params_manual.rs](../../examples/scoped_params_manual.rs) builds the same
tree with functions and the same execution assertions.
