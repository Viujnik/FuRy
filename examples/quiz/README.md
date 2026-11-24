# Quick Quiz

A small FastAPI app with a browser quiz, signup, sign-in and a leaderboard. PostgreSQL stores accounts, sessions and scores. The browser reads FuRy records through WASM. Two JSON endpoints expose the same leaderboard, one through a direct query and one through SQLAlchemy ORM.

## Run it

Use the build tools from the [main README](../../README.md) and install [uv](https://docs.astral.sh/uv/getting-started/installation/). Start from the repository root. Python 3.14, Node, `initdb` and `pg_ctl` must be on `PATH`.

```sh
bash examples/quiz/run.sh
```

Open the address printed by the script, normally `http://127.0.0.1:8001`. It builds the FuRy bindings, installs the example's locked Python dependencies in a separate uv environment and starts a temporary PostgreSQL server. Play the three questions, create a test account and save your score. Ctrl+C stops the processes and removes the test database. Use disposable passwords. This local app illustrates integration and needs deployment-specific authentication controls before public exposure.

The browser reads `/api/leaderboard.fury` and passes the page to the decoder. Game rules live in [questions.py](questions.py), routing and session handling in [app.py](app.py), and wire models in [models.py](models.py). To make a geography quiz, change `QUESTIONS`. Try `Question("Capital of Peru?", ("Lima", "Quito"), 0)`. Run `bash examples/quiz/run.sh smoke` to check submission and the leaderboard.

## Verify and compare

```sh
bash examples/quiz/run.sh smoke
bash examples/quiz/run.sh bench
bash examples/quiz/run.sh load
```

The smoke covers registration, duplicate usernames, password login, session logout, scoring and equality between JSON and FuRy. Set `QUIZ_BROWSER` to a Chrome executable to also verify rendering.

The benchmark warms the routes, rotates their order and verifies every decoded row. Direct JSON, Protobuf and Apache Fory share the same asyncpg query. The ORM route uses SQLAlchemy and FuRy uses SQLx. All three pools allow up to four connections. Timings include the database query, server encoding, loopback HTTP and decoding in Node. The sequential run also reports standalone client decode time and response-body size with and without offline gzip level 6. Set `QUIZ_BENCH_ITERATIONS=100` for 100 measured requests per route.

Load mode runs 1, 8 and 32 concurrent clients for five seconds per format and level. It adds successful requests per second, error count, p99 and transferred response-body MiB/s. Each client sends its next request after decoding and verifying the last response. Change the levels with `QUIZ_CLIENTS`, set `QUIZ_LOAD_SECONDS=15` for longer samples and use `QUIZ_REPEATS=3` to rotate run order over three repetitions. This measures the local read path with one Uvicorn process and a shared client/server machine.

`fury` writes the query result directly. `fury-models` creates a Python model for each row. The benchmark checks both responses byte for byte and compares decoded values across all codecs.

To include Protobuf and Apache Fory in the same runs, enable the optional codecs with npm on `PATH`.

```sh
QUIZ_COMPARE_CODECS=1 bash examples/quiz/run.sh bench
QUIZ_COMPARE_CODECS=1 QUIZ_LOAD_SECONDS=15 bash examples/quiz/run.sh load
```

The script installs locked benchmark dependencies in the temporary directory. Protobuf uses Python code generated from [leaderboard.proto](codecs/leaderboard.proto) and protobuf.js in Node. Apache Fory uses `pyfory` and `@apache-fory/core` in cross-language compatible mode, without the optional Node HPS accelerator.

The mixed fixture has 100,000 indexed rows with varied UTF-8 strings and signed integers within JavaScript's safe Number range. The transport tests cover the full signed 64-bit range separately. Use the large-page workload to test more data per request.

```sh
QUIZ_COMPARE_CODECS=1 QUIZ_DATASET=mixed QUIZ_PAGE_SIZE=1024 \
  QUIZ_CLIENTS=8,32,64 QUIZ_REPEATS=3 QUIZ_LOAD_SECONDS=10 bash examples/quiz/run.sh load
QUIZ_COMPARE_CODECS=1 QUIZ_DATASET=mixed QUIZ_PAGE_SIZE=1024 bash examples/quiz/run.sh profile
```

Profile mode measures database fetch, server encoding paths and client decoding. Its Python figures use tracemalloc and its Node figures sample V8 heap allocations. Native Rust, WASM linear memory and native codec allocations need native profilers.

## Order feed workload

`QUIZ_DATASET=orders` builds a deterministic, synthetic dataset of 250,000 orders, 20,000 customers and 64 shops. The feed joins customers to orders and uses the index on shop and descending order ID. Eleven response fields include exact amounts in cents, signed adjustments, timestamps in milliseconds, Unicode names and cities, statuses, optional coupons and delivery notes. A subset of notes exceeds 127 UTF-8 bytes. Clients request the same feed across formats, with more requests to popular shops.

```sh
for size in 50 250 1500; do
  QUIZ_DATASET=orders QUIZ_PAGE_SIZE="$size" QUIZ_COMPARE_CODECS=1 bash examples/quiz/run.sh bench
  QUIZ_DATASET=orders QUIZ_PAGE_SIZE="$size" QUIZ_COMPARE_CODECS=1 \
    QUIZ_CLIENTS=8,48 QUIZ_REPEATS=2 QUIZ_LOAD_SECONDS=6 bash examples/quiz/run.sh load
done
```

The three page sizes model a screen, a batch request and a large page. The loop runs 8.4 minutes of timed load plus setup, warmup and draining. Use `QUIZ_REUSE_BINDINGS=1` after the first build to skip rebuilding when running the remaining commands against the same code.

`json-direct` returns a pre-shaped scalar projection through Starlette's JSONResponse and uses the same SQL and asyncpg pool as Protobuf and Apache Fory. `json` shows FastAPI's ordinary return path. `orm.json` materializes SQLAlchemy order and customer entities. The benchmark verifies exact ordered values, including nulls, and checks both FuRy responses byte for byte. Load numbers cover the uncompressed local HTTP path. The sequential bench uses one representative page for the gzip size and standalone decode figures.

By default each command rebuilds the verified FuRy artifacts. For repeated runs of the same checkout, set `QUIZ_REUSE_BINDINGS=1` to use the existing artifacts under `target/bindings/current`.

## A real city directory

Download a [GeoNames cities500 snapshot](https://download.geonames.org/export/dump/readme.txt) once. The cities workload imports city names and population counts, using the quiz schema’s two fields for these values. The importer prints the row count and snapshot SHA-256. Keep the same ZIP for comparisons. [GeoNames](https://www.geonames.org/export/) publishes the data under CC BY 4.0.

```sh
curl -fL https://download.geonames.org/export/dump/cities500.zip -o /tmp/cities500.zip
export QUIZ_DATASET=cities QUIZ_CITIES_ZIP=/tmp/cities500.zip QUIZ_COMPARE_CODECS=1
QUIZ_PAGE_SIZE=1024 bash examples/quiz/run.sh profile
QUIZ_PAGE_SIZE=1024 QUIZ_CLIENTS=8,32 QUIZ_REPEATS=2 QUIZ_LOAD_SECONDS=10 bash examples/quiz/run.sh load
```
