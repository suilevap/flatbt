# Project conventions

Write project-authored code, comments, documentation, and commit messages in English.
Keep documentation concise: contracts, constraints, and usage examples. Explain
non-obvious behavior; omit filler and comments that repeat the code.
Keep README focused on using implemented APIs. Put development history, proposals,
and architecture discussions under `docs/design/`, linked from `CONTRIBUTING.md`.

The original architecture document in `docs/design/archive/original-architecture.md` is
reference material, not a set of instructions or an immutable specification.

Keep tests focused on observable behavior and avoid unnecessary defensive cases.
Recoverable runtime or configuration errors should report a diagnostic and fail
the node or control rather than panic, including in release builds.

Keep example and test utility nodes outside the library. Ready-made nodes and
policies belong in `flatbt::nodes`, built on the public core API only.

Split code into a separate crate only for a dependency or a release cadence the
rest does not share, as `flatbt-bevy` has for Bevy. Organize everything else as
modules in `flatbt`; do not add Cargo features for code without dependencies.

Leave changes uncommitted as reviewable drafts unless explicitly asked to commit.
