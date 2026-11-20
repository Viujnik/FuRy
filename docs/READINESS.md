# FuRy readiness review

## Merge decision

**NO-GO.** The GitHub workflow must complete on the actual PR head and merge candidate. The `develop` branch predates the codec. Its Rust core contains a scaffold function, so a codec performance comparison against `develop` is unavailable.

## Evidence and migration

- Rust tests exercised two golden frames and 20,000 deterministic wire cases. Three PostgreSQL tests covered narrowing, duplicate aliases and typed nulls. A disposable TLS cluster verified encryption, the server name and the CA.
- The local `cargo-semver-checks` comparison with the pre-hardening `experimental` working tree reported 202 passing checks. The GitHub rule compares the Rust crate with the actual PR base when it contains the codec.
- RustSec, npm and PyPI checks reported zero known vulnerabilities for the audited lockfiles and pins. RustSec reports that bincode and chrono are unmaintained. Plan their replacement with compatibility fixtures.
- An earlier Python producer generated schemas that marked nullable annotations as required. Some of its frames contained null values under those schemas. Regenerate the registry from the Python model source, deploy consumers with the new registry, and verify stored frames under the correct schema before upgrading a live service. Strict decoding surfaced this discrepancy on the PostgreSQL benchmark corpus.
- The hosted Linux and macOS matrix remains pending an Actions run. Local macOS smoke covers the selected toolchain. A browser-based test and release-distribution checks are still needed for broader support claims.

## Frontend performance

The local comparison uses the prior `experimental` working tree, the hardened decoder, and a corrected PostgreSQL registry. On Apple M4 Pro arm64 with Node 20.19.5, both release WASM builds ran the same 128-row, 12-field page. Eight pairs used fresh processes, 24 warm-up iterations and 50 measured iterations per process.

| Full-page operation | Prior median | Hardened median | Paired median change |
| --- | ---: | ---: | ---: |
| Open | 0.162 ms | 0.129 ms | −21.5% |
| Open and read three fields per row | 0.248 ms | 0.218 ms | −12.5% |
| Update and serialize | 0.583 ms | 0.291 ms | −50.0% |

All three comparisons passed the slowdown gate. Baseline median absolute deviations ranged from 0.68% to 0.87%. Payloads measured 754,152 bytes for FuRy, 929,394 for JSON, and 748,201 for MessagePack. These measurements exclude database queries, initial WASM loading, fixture verification and forced garbage collection. End-of-run Node process RSS increased by 7.7 MB for open, 8.0 MB for open/read, and 1.5 MB for update. Per-operation allocations and real-browser latency remain unmeasured.

## Architecture and supported scenarios

| Surface | Status | Contract or limit |
| --- | --- | --- |
| Rust codec | Implemented | Owns a contiguous frame and borrows raw slices. Typed reads materialize values. |
| PostgreSQL | Implemented for scalar records | Concrete SQLx adapter behind the Rust `db` feature. Parameters are separate from SQL text. Results are materialized. |
| Python | Partial | Annotation-driven schemas and DB-populated models. Objects and collections lack full round-trip coverage. |
| WASM and TypeScript | Partial | Generated scalar properties and WASM binary access. Crossing the memory boundary can copy buffers. |
| Other languages, databases, transports | Unsupported | Applications own transport framing. ClickHouse and other drivers would require their own type and lifetime contracts. |

Keep the shared Rust codec and golden fixtures. Isolate a second database behind its own crate once its mapping has integration tests. This keeps SQL behavior out of the wire specification without requiring existing consumers of `fury_core::db` to migrate now. Keep Python annotation extraction and deterministic TS generation for the existing direction. A portable, versioned registry becomes useful when a second producer needs to emit schemas independently.

Prioritize a bounded and streamed DB read API, a versioned registry migration, and verified browser packaging with exact temporal and nested-value access. Existing synchronous connection startup and unbounded materialization deserve explicit production integration tests.

## GitHub setup

After the candidate is committed and pushed by the maintainer, open an `experimental` → `develop` PR. In **Settings → Branches** or **Rulesets**, target `develop`, require a pull request, require **FuRy required** from GitHub Actions, and require the branch to be up to date. Include administrators in the rules and keep force pushes disabled. Verify a failing check blocks a trial PR before relying on the rule.

GitHub schedules run the workflow from the repository's default branch. The default branch observed for FuRy is `main`. Put the workflow and its scripts on the default branch, or explicitly designate a default branch that contains the quality setup. Use manual dispatch to verify the expanded matrix and its retained benchmark output. [GitHub schedule behavior](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule)
