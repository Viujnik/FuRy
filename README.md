# FuRy

Suppose your Python service has a page that lists users from PostgreSQL. The browser needs the user IDs and display names. With FuRy, the service can return those columns as binary data, and the browser can read them through WebAssembly. The rest of the application can keep its regular JSON routes.

FuRy is experimental. The examples exercise PostgreSQL, Python, and JavaScript or TypeScript. The application still decides who may see the users, how to page through them, and which format the client accepts.

## Run your first site

With the build tools listed below, uv and PostgreSQL installed, run this from the repository root:

```sh
bash examples/quiz/run.sh
```

Open the printed address, play a round and save your score. To change the topic, edit the [questions](examples/quiz/questions.py) and restart the example. You can follow your change from the browser form through to a stored score. [Setup, smoke test and codec benchmarks](examples/quiz/README.md)

## Follow one user from the database to the browser

Assume the table has three required columns, `id bigint`, `org_id bigint`, and `display_name text`. The endpoint exposes two of them. The class name supplies the schema name.

```python
from fury import BaseModel, FuryDB


class User(BaseModel):
    id: int
    display_name: str


async def user_page(db: FuryDB, authorized_org_id: int) -> bytes:
    return await db.fetch_page(
        User,
        """
        SELECT id, display_name FROM users
        WHERE org_id = $1 ORDER BY id LIMIT $2::bigint
        """,
        [authorized_org_id, 50],
        max_rows=50,
    )
```

Open the pool during service startup. After authorizing the request, return the bytes from the route. Publish the matching registry with the client.

```typescript
import init, { WasmRegistry } from "@fury/wasm-core";

await init();
const registryResponse = await fetch("/registry.bin");
if (!registryResponse.ok) throw new Error("Could not load the user schema");
const registry = new WasmRegistry(new Uint8Array(await registryResponse.arrayBuffer()));

const response = await fetch("/api/v1/users.fury");
if (!response.ok) throw new Error("Could not load users");

const users = registry.decodePage(
    "User", new Uint8Array(await response.arrayBuffer()), 50,
);
for (const user of users) console.log(user.id.toString(), user.display_name);
```

The page decoder returns an `id` as `bigint`, so a valid PostgreSQL ID such as `9007199254740993` arrives exactly. The SQL query keeps each page small, and the decoder applies its own byte and row limits.

## Run a complete example

The [browser walkthrough](examples/web/INTERNALS.md) serves three inspectable records. [Quick Quiz](examples/quiz/README.md) adds FastAPI, a browser interface, and a benchmark with JSON, Protobuf, and Apache Fory. Its order-feed workload also measures larger pages with a SQL join.

Use macOS or Linux, including WSL. Install Git, Rustup, Python 3.14, Node 22 with npm, and the PostgreSQL server tools. Put `initdb` and `pg_ctl` on `PATH`.

```sh
git clone --branch develop https://github.com/Viujnik/FuRy.git
cd FuRy
rustup toolchain install 1.94.1 --profile minimal --target wasm32-unknown-unknown
export RUSTUP_TOOLCHAIN=1.94.1
cargo install --locked wasm-pack --version 0.15.0
python3.14 -m venv .venv
source .venv/bin/activate
python -m pip install maturin==1.14.1
bash scripts/test-postgres-local.sh web-serve
```

The script builds the bindings, opens a temporary database, and prints a loopback URL. Open it in a browser and press **Ctrl+C** in the terminal when finished. Use `bash scripts/test-postgres-local.sh web` to run the automated round trip.

## What goes on the wire

A page contains the same self-contained frames you get from individual models. An empty page occupies zero bytes. A frame with just `id = 42` occupies 11 bytes, including its field name and a one-byte integer. A reader opens each frame independently and can retain unfamiliar fields through an edit.

Smaller responses and faster decoding are separate measurements. The order-feed example compares both under the same local workload, with raw sizes and gzip sizes. [Benchmark guide](benches/README.md)

Find the next step in the API guides for [Python](fury-py/README.md), [generation](fury-cli/README.md), [WASM](fury-ts/README.md), or [Rust and the wire format](fury-core/README.md). For repository work, see [Contributing](CONTRIBUTING.md), [Security](SECURITY.md), [Changelog](CHANGELOG.md), and the [Apache-2.0 license](LICENSE).
