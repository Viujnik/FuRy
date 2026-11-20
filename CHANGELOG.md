# Changelog

## Unreleased

- Add wire-v2 validation, legacy fixture compatibility and bounded frame decoding.
- Reject PostgreSQL narrowing overflow, ambiguous result aliases and incompatible SQL types for nullable columns.
- Build the concrete SQLx PostgreSQL driver with Rustls. Pin the workspace lockfile and update affected benchmark dependencies.
- Expose exact 64-bit integer access in WASM. Reject unsafe, fractional or out-of-range integer writes through JavaScript Number. Generated TypeScript realizes edits before serialization.
- Check clean Python and WASM installs, deterministic TypeScript generation, binary fixtures, interop, dependency advisories and benchmark scenarios in the quality workflow.
