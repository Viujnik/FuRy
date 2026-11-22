# FuRy

FuRy sends typed PostgreSQL rows from a Python service to a TypeScript client as binary records. Define a Python model, query a bounded page, and generate a TypeScript class and its schema registry. The client can read individual fields from each record and use `BigInt` for exact 64-bit integers.

FuRy is experimental and builds from source. The supported path is a bounded PostgreSQL read served by Python and decoded by a browser client. You can add one binary endpoint alongside your existing routes.

## Run the demo

Use macOS or Linux (including WSL). Install [Git](https://git-scm.com/install/), [Rustup](https://rustup.rs/), [Python 3.14](https://www.python.org/downloads/), [Node 22 with npm](https://nodejs.org/en/download) and the [PostgreSQL server tools](https://www.postgresql.org/download/). Make sure `initdb` and `pg_ctl` are on `PATH`. When `pg_config` is available, `export PATH="$(pg_config --bindir):$PATH"` adds the PostgreSQL binary directory.

```sh
git clone --branch experimental https://github.com/Viujnik/FuRy.git
cd FuRy
rustup toolchain install 1.94.1 --profile minimal --target wasm32-unknown-unknown
export RUSTUP_TOOLCHAIN=1.94.1
cargo install --locked wasm-pack --version 0.15.0
python3.14 -m venv .venv
source .venv/bin/activate
python -m pip install maturin==1.14.1
bash scripts/test-postgres-local.sh web-serve
```

In an existing checkout, begin with the toolchain command. Open the URL printed in the terminal. The script builds and installs the bindings, starts a disposable PostgreSQL cluster, and serves sample rows on loopback. Press **Ctrl+C** to stop it. The first build can take several minutes. To verify the pipeline without opening the page, run `bash scripts/test-postgres-local.sh web`.

## Use it in an existing Python service

Construct a `FuryDB` once during service startup. This example assumes `items` has a non-null integer `id` and non-null text `title`.

```python
from fury import BaseModel, FuryDB


class Item(BaseModel):
    id: int
    title: str


async def item_page(db: FuryDB) -> bytes:
    rows = await db.fetch_all(
        Item,
        "SELECT id::bigint AS id, title FROM items ORDER BY id LIMIT $1::bigint",
        [100],
    )
    return b"".join(row.to_bytes() for row in rows)
```

Return these bytes with `Content-Type: application/octet-stream`. The demo splits concatenated records by their wire-v2 length headers. Generate the model and registry together, then initialize the browser runtime with that registry. The application retains routing, access control, business validation and the database write path.

- [Python and PostgreSQL API](fury-py/README.md)
- [CLI and TypeScript generation](fury-cli/README.md)
- [WASM client and value types](fury-ts/README.md)
- [Rust codec and wire-v2 contract](fury-core/README.md)

## Integration limits

- `fetch_all` materializes the result. Bound result size in SQL. The Python binding uses its own PostgreSQL pool and exposes no transaction API for coordinating writes with another ORM.
- The Python producer builds records from query results. Arbitrary Python objects, decimals, and complete nested model support are outside the current binding contract. Map application-specific values explicitly before they reach the wire.
- Use a matching wheel, registry and WASM runtime. Regenerate pre-hardening registries for optional Python fields and check stored frames before upgrading.
- Other databases and languages need their own integration and compatibility tests.

## Benchmarks and development

The benchmark suite compares FuRy, JSON and MessagePack on identical Node page workloads. The Rust suite measures codec operations separately. See the [commands, fixtures and measurement scope](benches/README.md).

[Runtime walkthrough](examples/web/INTERNALS.md) · [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Changelog](CHANGELOG.md) · [Apache-2.0 license](LICENSE)
