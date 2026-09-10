# Post-commit action experiment

| Reference | Value |
| --- | --- |
| Branch | `experiment/post-commit-actions` |
| Commit | `95f9e332af6281e1916af25aea6aed0a86081804` |
| Base | `2f1525c21d6cdc993f4f0a95bb5cac0e1204b3d4` |

From a clean working tree:

```sh
git switch experiment/post-commit-actions
```

Includes code, design notes, tests, and examples. Only selected actions tick, using
transient typed targets, static target enums, and `Active<'node>` on `BtNode`.
Target dispatch uses no heap allocation or unsafe code. Archived because of API
complexity; main uses inline action callbacks.

Validation at commit: 26 behavior tests, one doctest, Clippy, and formatting passed.
An earlier standalone probe measured zero allocations across 2000 updates.
