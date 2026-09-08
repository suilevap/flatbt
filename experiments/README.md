# Post-commit action experiment

Branch: `experiment/post-commit-actions`.

Commit: `95f9e332af6281e1916af25aea6aed0a86081804`.

Base: `2f1525c21d6cdc993f4f0a95bb5cac0e1204b3d4`.

The branch preserves the full experiment, including design notes, tests, and
examples. Switch to it from a clean working tree to inspect or run that version:

```sh
git switch experiment/post-commit-actions
```

It preserves selected-only ticking with transient typed targets, static target
enums, and Active<'node> on BtNode. It uses no heap allocation for target dispatch
and no unsafe code. Review retained it as an experiment because of API complexity;
main uses the simpler inline action lifecycle.

Validation: 26 behavioral tests, one doctest, Clippy, and formatting checks passed
when the experiment was committed. The earlier standalone allocation probe also
measured zero allocations across 2000 updates.
