# Contributing

Start from the [repository README](README.md). Use the canonical checkout for implementation. Keep the Russian mirror aligned with the same known revision and compare behavior after translation.

## Checks for a pull request

```sh
rustup component add --toolchain 1.94.1 rustfmt clippy
bash scripts/check-rust.sh
bash scripts/test-postgres-local.sh bindings
bash scripts/test-postgres-local.sh web
```

For adapter changes also run `bash scripts/test-postgres-local.sh rust` and, when transport security is affected, `bash scripts/test-postgres-tls-local.sh`. For codec changes include a regression or golden fixture and run the extended corpus.

```sh
FURY_FUZZ_CASES=20000 cargo test --locked -p fury-core --no-default-features --test wire_contract
```

For performance changes use the [benchmark guide](benches/README.md). Report the workload, results and compatibility consequences in the PR.

Open integration PRs against `develop`. The GitHub check **FuRy required** combines the primary jobs. The target branch should require that check before merge. Scheduled jobs run from the repository default branch.

## Adding another language or database

Start with the [wire contract and fixtures](fury-core/README.md). A language binding can reuse the Rust codec. An independent codec needs tests that read and write the same fixtures and exercise both directions against Rust. For a database adapter first map one bounded typed result to a `BinaryRecord`. Then test nullability, ranges, ordering, page boundaries and errors against the actual driver.
