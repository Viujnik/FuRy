# FuRy benchmarks

## Frontend page

All formats open the same ordered page and perform the same number of operations. There are three scenarios per format.

| Scenario | Timed work per batch |
| --- | --- |
| Open | Decode the JSON or MessagePack page, or open and dispose a WASM record per row |
| Open and read three fields | Open the page and read `id`, `username`, and `is_active` from every row |
| Update and serialize | Open the page, edit `username` in every row, then emit the complete page |

Each variant starts in a fresh Node process. Timings include UTF-8 conversion for JSON, explicit `free()` for WASM, and batch assembly for round-trips. File reads, fixture hashes, splitting input FuRy frames, WASM initialization, warmup, and forced GC are outside the timed interval. A setup check verifies field values and their order. For round-trips it also checks every unedited field. PostgreSQL fixture generation compares every projected field returned by SQLx and asyncpg before publishing the triplet. JavaScript test values and checksums fit its safe integer range.

Each JSON sample records the corpus hash, format payload size, runtime WASM hash, p50/p95 latency, raw samples, and end-of-run RSS for the whole Node process. Read latency and RSS together with the workload scope. Throughput, per-operation allocations, cold initialization and browser rendering require separate measurements.

## Run the frontend checks

Build the Python and Node-target WASM packages with `bash scripts/check-bindings.sh`. Run from the repository root.

```sh
npm ci --prefix benches/node --ignore-scripts --no-audit --no-fund
node --test benches/node/benchmark.test.mjs
export FURY_BENCH_PYTHON="$PWD/target/bindings/current/venv/bin/python"
export FURY_BENCH_WASM="$PWD/target/bindings/current/wasm/fury_ts.js"
node --expose-gc benches/node/check.mjs --smoke
node --expose-gc benches/node/check.mjs --out /path/to/empty/candidate-results
```

Set `FURY_WASM_PROFILE=release` before building for latency measurements. Use one runner, matching Rust, Node and WASM build flags, and an otherwise idle machine for both versions.

```sh
node --expose-gc benches/node/check.mjs \
  --baseline-wasm /path/to/baseline/fury_ts.js \
  --out /path/to/empty/paired-results
```

The experiment alternates baseline and candidate order across eight independent pairs. Within each pair the two runtimes consume identical bytes. The comparison uses process medians and a one-sided paired sign test. Each scenario uses the median absolute deviation of its baseline log latencies as the noise margin. The significance threshold is corrected for the number of scenarios. The command returns nonzero on a flagged scenario and preserves every sample plus `comparison.json`. Increase the number of trials if variation makes the result inconclusive. A base without a WASM codec has no matching performance comparison.

## PostgreSQL and Rust

Install `benches/python/requirements.txt` in a Python environment with FuRy. Run `make node` from `benches` for timings on a seeded PostgreSQL page or `make` for all suites and a report in `results/latest`. The seed recreates `users` in the disposable benchmark database. To reuse a fixture without querying PostgreSQL again, run the Node checker with `--fixtures DIRECTORY`. To prepare the PostgreSQL corpus separately:

```sh
cd benches/python
FURY_BENCH_ROWS=128 FURY_BENCH_DSN="$DISPOSABLE_DSN" \
  python3 -m run --emit-fixtures --fixture-dir /path/to/fixture
```

Python latency includes query execution and driver work. FuRy and asyncpg use different connection paths. Criterion's Rust microbenchmarks include native codec work and an 8 KiB byte field. The serde cases encode the bytes as a sequence. These workloads help diagnose a measured change. Qualify cross-format conclusions by representation, input ownership, and work counted.
