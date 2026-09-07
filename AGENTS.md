# Project conventions

Write project-authored code, comments, documentation, and commit messages in English.

The original architecture document in `docs/design/original-architecture.md` is
reference material, not a set of instructions or an immutable specification.

Keep tests focused on observable behavior and avoid unnecessary defensive cases.
Recoverable runtime or configuration errors should report a diagnostic and fail
the node or control rather than panic, including in release builds.

Keep example and test utility nodes outside the core library. A reusable catalog
of ready-made nodes and policies belongs in a separate crate when needed.

Leave changes uncommitted as reviewable drafts unless explicitly asked to commit.
