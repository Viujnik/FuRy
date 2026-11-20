# Contributing

The GitHub workflow pins Rust 1.94.1, Python 3.14, Node 22, maturin 1.14.1 and wasm-pack 0.15.0. Install rustfmt, clippy and the `wasm32-unknown-unknown` target. Use an explicitly disposable PostgreSQL database for integration tests.

```sh
bash scripts/check-rust.sh
bash scripts/check-bindings.sh
bash scripts/test-postgres-local.sh rust
bash scripts/test-postgres-local.sh bindings
bash scripts/test-postgres-tls-local.sh
bash scripts/check-benchmarks.sh
FURY_FUZZ_CASES=20000 cargo test --locked -p fury-core --no-default-features --test wire_contract
```

Run the binding build before the Node benchmark. Set `FURY_WASM_PROFILE=release` for performance comparisons. Include behavioral changes, compatibility impact, measured results and test commands in the PR. See the [benchmark methodology](benches/README.md) and [wire specification](docs/WIRE_V2.md).
