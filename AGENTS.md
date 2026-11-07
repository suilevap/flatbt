# Project conventions

Write project-authored code, comments, documentation, and commit messages in English.
Keep documentation concise: contracts, constraints, and usage examples. Explain
non-obvious behavior; omit filler and comments that repeat the code.
Keep README focused on using implemented APIs. Put development history, proposals,
and architecture discussions under `docs/design/`, linked from `CONTRIBUTING.md`.

The original architecture document in `docs/design/archive/original-architecture.md` is
reference material, not a set of instructions or an immutable specification.

Name type parameters by role, not with single letters: `Blackboard`, `Act`,
`Params`, `Child`, `ChildState`, `Condition`. A `const` length is upper case,
such as `DEPTH`.

The public API separates thinking from acting. `Blackboard` is the agent's mutable
perception and working memory. Every node, control, action, order, and scope
builder follows `BtNode`'s contract: visited branches keep their blackboard and memory
writes, including branches that fail or are replaced. `Failure` is an outcome,
not a rollback. The driver applies only the final returned `Act` to the world.
Keep this distinction visible in API docs and examples; examples that change
gameplay state should do so in the driver. See
[`docs/design/thinking-and-acting.md`](docs/design/thinking-and-acting.md).

Examples separate observed facts from reasoning. Gather refreshes perception
from gameplay state; tree callbacks record considered options, estimates, and
plans without simulating an act by changing observed ammunition or position.
Keep the world and the AI blackboard separate. After applying an act, refresh
observations to report what actually happened, preserving useful reasoning.

Keep tests focused on observable behavior and avoid unnecessary defensive cases.
Recoverable runtime or configuration errors should report a diagnostic and fail
the node or control rather than panic, including in release builds.

Keep example and test utility nodes outside the library. Ready-made nodes and
policies belong in `flatbt::nodes`, built on the public core API only.

Split code into a separate crate only for a dependency or a release cadence the
rest does not share, as `flatbt-bevy` has for Bevy. Organize everything else as
modules in `flatbt`. Cargo features stay two, both on by default: `extras`
(`nodes` and `scope`) and `std`. Library code uses `core`; only `std`-gated
diagnostics use `std`. Keep new optional code under `extras` rather than adding
a feature, unless it brings a dependency.

Leave changes uncommitted as reviewable drafts unless explicitly asked to commit.
