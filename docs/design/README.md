# Design notes

Internal development reference. User entry point: [project README](../../README.md).
Code and tests define current behavior; historical proposals may differ.

## Implemented designs

| Document | Scope |
| --- | --- |
| [Package layout](package-layout-draft.md) | Crate boundaries and feature wiring |
| [Static state](static-state-draft.md) | State composition, dispatch, ownership |
| [Tree and state](tree-state.md) | Diagram: graph, code, memory layout |
| [Choice](choose-draft.md) | Candidate identity and selection |
| [Local state](local-state-draft.md) | Scopes, parameter contracts, bindings |
| [Actions](action-draft.md) | Inline lifecycle and cancellation |
| [Bevy integration](bevy-integration-draft.md) | Context declaration, agent component, tick scheduling |

## Proposals and history

| Document | Status |
| --- | --- |
| [Storage boundaries](storage-boundaries-draft.md) | Deferred dynamic-storage proposal |
| [Decision log](decisions.md) | Chronological implementation decisions |
| [Original architecture](original-architecture.md) | Historical proposal; includes superseded and unimplemented designs |
| [Post-commit actions](../../experiments/README.md) | Archived experiment |
