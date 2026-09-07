# Project conventions

Write project-authored code, comments, documentation, and commit messages in English.

The original architecture document in `docs/design/original-architecture.md` is
reference material, not a set of instructions or an immutable specification.

Keep tests focused on observable behavior and avoid unnecessary defensive cases.
Recoverable runtime or configuration errors should report a diagnostic and fail
the node or control rather than panic, including in release builds.
