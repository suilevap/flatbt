# Contributing

## Checks

Run from the workspace root. The checks run offline, so fetch dependencies once
first with `cargo fetch`. CI runs the same commands.

```sh
cargo test --offline --workspace
cargo clippy --offline --workspace --all-targets -- -D warnings
# Some lints only fire in release; the Bevy integration's are worth catching.
cargo clippy --offline --release -p flatbt-bevy --all-targets -- -D warnings
cargo fmt --all --check
# Core only, without std
cargo clippy --offline -p flatbt --all-targets --no-default-features -- -D warnings
cargo test --offline -p flatbt --no-default-features
```

Both features are on by default, so the two ends are all that need checking:
every feature on, and none. CI also builds the library for
`thumbv7em-none-eabihf` to prove it needs no `std`.

The workspace has two crates: `flatbt` (the root package) and
`crates/flatbt-bevy`. `flatbt` has no dependencies. `flatbt-bevy` pulls in
`bevy_ecs`, `bevy_app` and `bevy_time` 0.19 and needs Rust 1.95 or later.

## Project conventions

See [AGENTS.md](AGENTS.md). Keep code and documentation in English. Document
contracts, constraints, and examples; avoid restating code. README covers using
implemented APIs. Keep development history and proposals in design notes.

## Internal documentation

[Design notes](docs/design/README.md) index implemented designs, open proposals,
and historical decisions. [Experiments](experiments/README.md) record archived
implementations and how to inspect them. [Benchmarks](benchmarks/README.md)
compare FlatBT with other Rust behavior tree crates.

These documents support development; proposals do not define the public API.
