# Contributing to FuRy

Start by running the [browser example](README.md#run-a-complete-example). It gives you a working request to follow from SQL to the browser. Open integration pull requests against `develop`. The **FuRy required** check collects the main CI results.

## Show the behavior your change affects

Start with a case you can describe in a sentence. For example, if a new reader is meant to accept a page written by the previous release, save that earlier page as a fixture and show which fields the new reader returns. Run the existing wire tests alongside it.

```sh
rustup component add --toolchain 1.94.1 rustfmt clippy
bash scripts/check-rust.sh
bash scripts/test-postgres-local.sh bindings
bash scripts/test-postgres-local.sh web
```

Use `bash scripts/test-postgres-local.sh rust` for adapter tests and `bash scripts/test-postgres-tls-local.sh` for PostgreSQL TLS changes. The TLS test exercises peer verification with a disposable database.

For codec changes, keep a small fixture with known bytes and values. The extended wire-contract test exercises more combinations.

```sh
FURY_FUZZ_CASES=20000 cargo test --locked -p fury-core --no-default-features --test wire_contract
```

## Compare before and after on the same data

If you remove a copy, benchmark the path that used it. For example, feed the old and new readers the same page and have both read the same fields. Then run the endpoint benchmark to see whether the change matters once SQL and HTTP are involved. Put the workload, build profile, machine, and observed results in the pull request. [Benchmark commands](benches/README.md)

## Add a driver or language in one round trip

For a database adapter, start with a query that returns a string, a number, and a nullable value. Check that a wrong SQL type or a value outside the target range gives an error. For another language, first read a Rust-produced fixture, produce the same logical value, and have Rust read it back. [Wire contract and fixtures](fury-core/README.md)

The English checkout is the reference for changes. When updating the Russian mirror, keep the examples, commands, and intended behavior in sync.
