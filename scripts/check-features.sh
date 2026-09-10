#!/bin/sh
# Run at the workspace root. Separate invocations prevent feature unification
# across workspace members from hiding missing feature declarations.
set -eu

for features in '' action choose scope action,choose action,scope choose,scope action,choose,scope; do
    cargo test --offline -p flatbt --no-default-features --features "$features"
done

cargo test --offline -p flatbt-core
cargo test --offline -p flatbt-nodes --no-default-features --features choose
cargo test --offline -p flatbt-nodes --no-default-features --features action
cargo test --offline -p flatbt-scope
