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
Bevy 0.20 and needs Rust 1.95 or later.

## Bevy release lines

`flatbt-bevy` 0.1.x targets Bevy 0.19; 0.2.x targets Bevy 0.20. Both use the
same Rust sources, tests, and examples. The adapter has its own package version;
`flatbt` stays at 0.1.x when the Bevy version changes. CI checks both adapters.

Use Python 3.11 or later to prepare a release in a clean release worktree. For
example, to prepare a Bevy 0.19 patch release from the current sources:

```sh
python3 scripts/prepare-bevy-release.py 0.1.1
cargo fetch
```

Then run the checks above. The script updates the adapter's version and all its
Bevy requirements, including the multithreaded ECS dev dependency. `cargo fetch`
updates the lockfile. It does not copy or change Rust sources. Select a 0.2.x
version to prepare the Bevy 0.20 line instead. The checked-in manifest and
lockfile target Bevy 0.20. Packages remain unpublished.

## Project conventions

See [AGENTS.md](AGENTS.md). Keep code and documentation in English. Document
contracts, constraints, and examples; avoid restating code. README covers using
implemented APIs. Keep development history and proposals in design notes.

## Internal documentation

[Design notes](docs/design/README.md) index implemented designs, open proposals,
and historical decisions. [Experiments](experiments/README.md) record archived
implementations and how to inspect them.

These documents support development; proposals do not define the public API.
