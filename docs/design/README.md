# Design notes

Internal development reference. User entry point: [project README](../../README.md).
Code and tests define current behavior; historical proposals may differ.

## Implemented designs

| Document | Scope |
| --- | --- |
| [Package layout](package-layout.md) | Two crates, and what goes in each |
| [Static state](static-state.md) | State composition, dispatch, ownership |
| [Tree and state](tree-state.md) | Diagram: graph, code, memory layout |
| [Choice](choose.md) | Candidate identity and selection |
| [Local state](local-state.md) | Scopes, parameter contracts, bindings |
| [Actions](action.md) | Inline lifecycle and cancellation |
| [Bevy integration](bevy-integration.md) | Blackboard as a component, tree resource, tick plugin |
| [Inspection](inspect.md) | Node names and text views of running state |
| [Tree memory](tree-memory-draft.md) | Per-node state that outlives a run, owned beside the run state |

## Proposals

| Document | Status |
| --- | --- |
| [Goal stack](goal-stack-draft.md) | Goals that need other goals: a stack, one goal running at a time; first implementation |
| [Node catalog](node-catalog-draft.md) | Decorators, helpers, and selection policies; being implemented |
| [Trace](trace.md) | Why the tree chose what it runs: a typed per-agent log in dev builds, merged by `inspect` |

## Archive and history

| Document | Status |
| --- | --- |
| [Storage boundaries](archive/storage-boundaries.md) | Deferred dynamic-storage proposal |
| [Decision log](decisions.md) | Chronological implementation decisions |
| [Original architecture](archive/original-architecture.md) | Historical proposal; includes superseded and unimplemented designs |
| [Post-commit actions](../../experiments/README.md) | Archived experiment |
