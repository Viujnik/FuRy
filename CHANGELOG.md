# Changelog

## Unreleased

- Add an order-feed example that compares small and large pages with JSON, Protobuf, and Apache Fory.
- Write wire v3 for both records and pages. Automatically shorten integers and short strings. Read v2 frames for migration. Add a GeoNames benchmark.
- Bind Python queries to the schema on the supplied class. Add namespaced schema IDs and isolated WASM registries with exact bigint page values.

- Add a runnable PostgreSQL-to-browser example and verify it in the interoperability job.
- Validate code generation destinations before writing and replace each generated file atomically. Reject invalid registry and model source paths.
- Add wire-v2 validation, legacy fixture compatibility and bounded frame decoding.
- Reject PostgreSQL narrowing overflow, ambiguous result aliases and incompatible SQL types for nullable columns.
- Build the concrete SQLx PostgreSQL driver with Rustls. Pin the workspace lockfile and update affected benchmark dependencies.
- Expose exact 64-bit integer access in WASM. Reject unsafe, fractional or out-of-range integer writes through JavaScript Number. Generated TypeScript realizes edits before serialization.
- Check clean Python and WASM installs, deterministic TypeScript generation, binary fixtures, interop, dependency advisories and benchmark scenarios in the quality workflow.
