# Package layout draft

## Boundaries

FlatBT is a Cargo workspace with a small entry-point package and three library
packages. A crate is the dependency boundary; modules organize code within that
boundary; features select optional capabilities. No external dependencies are added.

```text
Cargo.toml                       workspace and flatbt feature wiring
src/lib.rs                       flatbt re-exports
crates/
  flatbt-core/
    build.rs                     tuple states, parameter shapes, child indices
    src/
      lib.rs
      runtime/                   BtNode, NodeResult, EntryMode, BtState, update
      composition/               child dispatch, generic controls, basic primitives
      params.rs                  parameter shapes and reborrowing
  flatbt-nodes/
    src/
      lib.rs
      action/                    lifecycle adapter and cancel-on-drop resources
      choose.rs                  context-driven choice policy, node, and macro
  flatbt-scope/
    src/
      lib.rs
      storage.rs                 owned invocation locals and initializers
      binding.rs                 parameter projections and adapters
      macros.rs                  scope DSL
examples/                        application examples and shared utility nodes
tests/                           public API behavior and cross-package composition
scripts/check-features.sh         isolated feature matrix and direct package checks
```

Dependencies point inward:

```text
flatbt --always----------------------> flatbt-core
       --feature choose or action---> flatbt-nodes ---> flatbt-core
       --feature scope--------------> flatbt-scope ---> flatbt-core
```

Core provides execution, static child dispatch, parameter contracts, and basic
composition (`control`, `seq`, `select`, `leaf`, `check`). These primitives make a
small tree useful without opting into another package. Parameter reborrowing is
used by generic controls and is not tied to the scope DSL.

The action lifecycle is an optional adapter over `BtNode`, with cancellation
owned by its state. It lives with ready-made nodes rather than in the execution
kernel. `scope` and `choose` use only public core APIs and do not depend on one
another or on actions. Core has no dependency on the entry point or extensions.

Future priority/random policies and reusable utility nodes belong in
`flatbt-nodes`, organized by behavior in modules. Add independent features when
selective compilation or optional dependencies are useful; a new crate for every
node is unnecessary. Keep application-specific and test nodes in example/test
support. No speculative priority/random implementation is introduced by this draft.

## Using the packages

The default `flatbt` dependency exposes core only:

```toml
[dependencies]
flatbt = { path = "../FlatBT" }
```

Select any combination of helpers:

```toml
[dependencies]
flatbt = { path = "../FlatBT", features = ["choose", "scope", "action"] }
```

```rust,ignore
use flatbt::{BtNode, BtState, choose, action};
use flatbt::scope::{scope, bind, read};
```

Direct dependencies are equally supported, without the entry-point package:

```toml
[dependencies]
flatbt-core = { path = "../FlatBT/crates/flatbt-core" }
flatbt-nodes = { path = "../FlatBT/crates/flatbt-nodes", features = ["choose"] }
flatbt-scope = { path = "../FlatBT/crates/flatbt-scope" }
```

```rust,ignore
use flatbt_core::{BtNode, BtState, update};
use flatbt_nodes::choose;
use flatbt_scope::{scope, bind, read};
```

All paths above are relative to a consuming application's manifest and must be
adapted to its checkout. Packages remain unpublished. Both the entry point and
`flatbt-nodes` have empty default feature sets. Features are additive: if another
dependency enables a helper on the same package, Cargo unifies that feature.
Depending directly on `flatbt-core` provides the strictest dependency boundary.

## Migration and implementation details

Existing core import paths are preserved. Existing root `choose!`, `ChooseNode`,
`BtAction`, and cancellation imports work after enabling the corresponding
features. Scope paths remain `flatbt::scope::*` after enabling `scope`. Code using
all the previous helpers must add `features = ["choose", "scope", "action"]`.
This is an intentional default-feature/API availability change in an unpublished
experimental package, not a promise of backward compatibility.

`ChooseNode` is now a wrapper around `ControlNode<Choose<F>, Children>` instead of
a type alias. Rust does not allow a downstream crate to add inherent constructors
to a foreign type. The wrapper preserves `ChooseNode::new`, delegates execution,
and uses the same associated state. Code relying on assignment interchangeability
with the former alias must use `control(Choose(chooser), children)` or construct a
`ChooseNode` explicitly. No state fields or allocations were added.

Only core's build script reads `FLATBT_MAX_CHILDREN` and `FLATBT_MAX_PARAMS`.
A hidden generic macro supplies generated child indices to the catalog's macro;
there is no second arity setting that can drift. Exported macros use `$crate`
and hidden re-exports of their dependencies, so users need not import internal
helpers and may rename packages in Cargo.toml.

Behavioral tests and examples continue to exercise the public entry point.
Cargo `required-features` skips targets whose helper dependencies are absent.
The existing scope integration suite also uses actions, so that target requires
both features; scope's own doctests exercise its independent API. The feature
check script builds each combination in a separate Cargo invocation, including
core-only and direct package usage, to avoid workspace feature unification masking
wiring errors. The complete suite uses `--workspace --all-features`.

## Rust references

- [The Rust Book: separating modules into files](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html): files organize the module tree; public re-exports can keep a convenient API.
- [Cargo Book: workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html): related packages share a lockfile, target directory, and common manifest settings.
- [Cargo Book: features](https://doc.rust-lang.org/cargo/reference/features.html): optional dependencies and additive feature selection; feature unification means feature flags are not an isolation boundary.

The specific core/catalog/scope split is this project's design choice, not a Rust
requirement. It keeps the optional helpers independently usable while avoiding a
separate package for each small policy.
