# Contributing

## Checks

Run from the workspace root:

```sh
cargo test --offline --workspace --all-features
cargo clippy --offline --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
sh scripts/check-features.sh
```

The feature script checks defaults, explicit feature combinations, and direct
crates in separate Cargo invocations.

`flatbt-bevy` pulls in `bevy_ecs` and `bevy_app` 0.19 and needs Rust 1.95 or
later. The other crates have no dependencies.

## Project conventions

See [AGENTS.md](AGENTS.md). Keep code and documentation in English. Document
contracts, constraints, and examples; avoid restating code. README covers using
implemented APIs. Keep development history and proposals in design notes.

## Internal documentation

[Design notes](docs/design/README.md) index implemented designs, open proposals,
and historical decisions. [Experiments](experiments/README.md) record archived
implementations and how to inspect them.

These documents support development; proposals do not define the public API.
