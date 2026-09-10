#!/bin/sh
# Run from the workspace root. Separate calls prevent feature unification
# from hiding missing declarations.
set -eu

cargo test --offline -p flatbt

for features in '' action choose scope action,choose action,scope choose,scope action,choose,scope; do
    cargo test --offline -p flatbt --no-default-features --features "$features"
done

cargo test --offline -p flatbt-core
cargo test --offline -p flatbt-nodes --no-default-features --features choose
cargo test --offline -p flatbt-nodes --no-default-features --features action
cargo test --offline -p flatbt-scope
