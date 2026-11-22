#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo test --locked -p fury-core --no-default-features
cargo run --locked -p fury-core --no-default-features --example roundtrip
RUSTDOCFLAGS=-Dwarnings cargo doc --locked --workspace --lib --no-deps
for script in scripts/*.sh benches/scripts/*.sh; do
    bash -n "$script"
done
