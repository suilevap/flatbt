# Package layout

Implemented Cargo workspace. No external dependencies.

```text
flatbt --always----------------------> flatbt-core
       --feature choose or action---> flatbt-nodes ---> flatbt-core
       --feature scope--------------> flatbt-scope ---> flatbt-core
```

## Ownership

| Location | Responsibility |
| --- | --- |
| `src/lib.rs` | Entry-point re-exports and feature gates |
| `crates/flatbt-core/src/runtime/` | BtNode, NodeResult, EntryMode, BtState, update |
| `crates/flatbt-core/src/composition/` | Static dispatch, custom controls, seq, select, leaf, check |
| `crates/flatbt-core/src/params.rs` | Parameter shapes and reborrowing |
| `crates/flatbt-core/build.rs` | Tuple states, parameter tuples, child indices |
| `crates/flatbt-nodes/src/action/` | Lifecycle and cancellation adapters |
| `crates/flatbt-nodes/src/choose.rs` | Choice policy, wrapper, macro |
| `crates/flatbt-scope/src/storage.rs` | Owned locals and initializers |
| `crates/flatbt-scope/src/binding.rs` | Parameter projections and adapters |
| `crates/flatbt-scope/src/macros.rs` | Scope DSL |
| `examples/`, `tests/` | Application examples, test helpers, public API tests |
| `scripts/check-features.sh` | Isolated feature and direct-package checks |

Core has no dependency on optional helpers. Scope and choose use public core
APIs independently of each other and actions. Reusable policies/nodes belong in
`flatbt-nodes`, grouped by behavior; use independent features where useful.
Application-specific and test nodes stay outside library crates.

## Dependencies

Entry point with all helpers (default):

```toml
[dependencies]
flatbt = { path = "../FlatBT" }
```

Core with selected helpers:

```toml
[dependencies]
flatbt = { path = "../FlatBT", default-features = false, features = ["choose", "scope"] }
```

Direct crates:

```toml
[dependencies]
flatbt-core = { path = "../FlatBT/crates/flatbt-core" }
flatbt-nodes = { path = "../FlatBT/crates/flatbt-nodes", features = ["choose"] }
flatbt-scope = { path = "../FlatBT/crates/flatbt-scope" }
```

Paths are relative to the consuming manifest. Packages are unpublished.
`flatbt` enables choose, scope, and action by default. Set `default-features = false`
for core only; add individual features as needed. `flatbt-nodes` has no default
features. Cargo unifies features across consumers; depend directly on core for a
strict dependency boundary.

## Migration

Core imports keep their root paths. Feature `choose` supplies root `choose!`/`ChooseNode`,
`action` for root action/cancellation exports, and `scope` for `flatbt::scope::*`.
The entry point enables all three by default. Core-only consumers must opt out.

ChooseNode changed from alias to wrapper around
`ControlNode<Choose<F>, Children>`: Rust forbids adding inherent constructors to a
foreign type. The wrapper keeps `ChooseNode::new` in the catalog crate and delegates
execution with unchanged associated state. No added state fields or allocations.
Code relying on alias interchangeability must construct ChooseNode explicitly or
use `control(Choose(chooser), children)`.

## Macro and feature checks

Only core reads `FLATBT_MAX_CHILDREN` and `FLATBT_MAX_PARAMS`. A hidden macro passes
its generated indices to `choose!`. Exported macros use `$crate` and hidden dependency
re-exports, including when consumers rename dependencies.

Tests/examples use the public entry point. Cargo `required-features` skips targets
without their helpers. Scope integration tests require scope + action; scope
doctests also exercise the independent crate. Run `scripts/check-features.sh` for
separate Cargo invocations so workspace feature unification cannot mask missing
declarations. Run the full suite with `--workspace --all-features`.

## Rust references

[Modules and files](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html) ·
[Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html) ·
[Features](https://doc.rust-lang.org/cargo/reference/features.html)
