#!/usr/bin/env python3
"""
Entry point for the Python benchmark.

Usage:
    python -m run --variant asyncpg_to_json_orjson --out results.json
    python -m run --emit-fixtures

Design:
    One process = one variant. run_all.sh loops over variants and spawns a
    fresh interpreter for each. This is the only way to guarantee that JIT
    state, GC freelists, and prepared-statement caches from one variant don't
    leak into another.

    `--emit-fixtures` produces the byte fixtures consumed by the Node bench:
        fixtures/wide.json
        fixtures/wide.msgpack
        fixtures/wide.fury
    These are the exact payloads a server would put on the wire.
"""


import argparse
import asyncio
import hashlib
import json
import os
import struct
from pathlib import Path

from harness import run_bench
from variants import ALL_VARIANTS, build, WIDE_QUERY, _json_default, _msgpack_default

DSN = os.environ.get(
    "FURY_BENCH_DSN",
    "postgresql://fury:fury@127.0.0.1:54329/fury_bench",
)

ROWS = int(os.environ.get("FURY_BENCH_ROWS", "1000"))

# Iterations are set high enough that the p95 stabilizes. On an M4 Pro
# this is about 3 seconds of wall time per variant, which is fine.
WARMUP = 20
ITERATIONS = 50


# ────────────────────────────────────────────────────────────────────────
# Fixture generation (Node bench input)
# ────────────────────────────────────────────────────────────────────────

async def emit_fixtures(destination: Path | None) -> None:
    """Emit the same ordered PostgreSQL rows in all three formats."""
    import asyncpg, orjson, msgpack

    fixtures = destination or (Path(__file__).resolve().parent.parent / "fixtures")
    fixtures = fixtures.resolve()
    fixtures.mkdir(parents=True, exist_ok=True)

    # Support the private Unix socket DSNs used by disposable local clusters.
    from urllib.parse import parse_qs, urlsplit
    socket_host = parse_qs(urlsplit(DSN).query).get("host", [None])[0]
    conn = await asyncpg.connect(DSN, host=socket_host)
    try:
        rows = await conn.fetch(WIDE_QUERY, ROWS)
        dicts = [dict(r) for r in rows]
    finally:
        await conn.close()

    # Fury fixture: use the same DB driver path as the actual benchmark, so
    # the bytes are exactly what a fury server would send.
    from fury import FuryDB, BaseModel
    from datetime import datetime
    from tempfile import TemporaryDirectory
    from uuid import UUID

    # Register into a fresh directory so a prior benchmark's schema cache
    # cannot contaminate the bytes published with this corpus.
    cwd = Path.cwd()
    with TemporaryDirectory(prefix="fury-bench-schema-") as registry_dir:
        os.chdir(registry_dir)
        try:
            class BenchUser(BaseModel):
                id: int
                username: str
                email: str
                display_name: str | None
                is_active: bool
                reputation: int
                flags: int
                external_id: UUID
                created_at: datetime
                updated_at: datetime
                bio: str | None
                avatar: bytes | None

            db = FuryDB.connect(DSN)
            models = await db.fetch_all(BenchUser, WIDE_QUERY, [ROWS])
            registry = (Path(registry_dir) / ".fury_registry.bin").read_bytes()
        finally:
            os.chdir(cwd)

    if not dicts or len(models) != len(dicts):
        raise RuntimeError("Fixtures require a nonempty, equal row count")
    checksum, checksum_bound = 0, 0
    for index, (expected, model) in enumerate(zip(dicts, models)):
        for field, value in expected.items():
            actual = getattr(model, field)
            if isinstance(value, UUID):
                value, actual = str(value), str(actual)
            if actual != value:
                raise RuntimeError(f"Fixture mismatch at row {index}, field {field}")
        # JavaScript measures string length in UTF-16 code units.
        name_length = len(expected["username"].encode("utf-16-le")) // 2
        active = int(expected["is_active"])
        checksum += expected["id"] + name_length + active
        checksum_bound += abs(expected["id"]) + name_length + active
    if checksum_bound > 2**53 - 1:
        raise ValueError("read3 checksum requires JavaScript-safe integers")

    blobs = [model.to_bytes() for model in models]
    for blob in blobs:
        if len(blob) < 6 or struct.unpack_from("<I", blob)[0] != len(blob) or blob[4:6] != b"F\x03":
            raise RuntimeError("Invalid wire-v3 fixture frame")

    files = {
        "wide.fury": b"".join(blobs),
        "wide.json": orjson.dumps(dicts, default=_json_default),
        "wide.msgpack": msgpack.packb(dicts, default=_msgpack_default),
        ".fury_registry.bin": registry,
    }
    for name, data in files.items():
        (fixtures / name).write_bytes(data)
    metadata = {
        "fixture_version": 2,
        "schema_name": "BenchUser",
        "fury_layout": "concatenated-v3-frames",
        "order_by": "id ASC",
        "rows": len(dicts),
        "fields": list(dicts[0]),
        "read3_checksum": checksum,
        "sha256": {name: hashlib.sha256(data).hexdigest() for name, data in files.items()},
    }
    (fixtures / "metadata.json").write_text(json.dumps(metadata, indent=2))

    print(f"→ fixtures written to {fixtures}")
    for f in sorted(fixtures.glob("wide.*")) + [fixtures / ".fury_registry.bin"]:
        if f.exists():
            print(f"   {f.name:20s} {f.stat().st_size:>10,} bytes")
# ────────────────────────────────────────────────────────────────────────
# Single-variant run
# ────────────────────────────────────────────────────────────────────────

async def run_one(variant: str, out_path: Path) -> None:
    # build() открывает соединение внутри себя — setup отдельно не нужен.
    name, timed, cleanup = await build(DSN, variant, n=ROWS)
    try:
        result = await run_bench(name, timed, warmup=WARMUP, iterations=ITERATIONS)
    finally:
        await cleanup()

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(result.as_dict(), indent=2))
    print(f"  {name}: median={result.median_ms:.2f} ms  p95={result.p95_ms:.2f} ms  "
          f"bytes_out={result.bytes_out}")


# ────────────────────────────────────────────────────────────────────────
# CLI
# ────────────────────────────────────────────────────────────────────────

def main():
    p = argparse.ArgumentParser()
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--variant", choices=ALL_VARIANTS)
    g.add_argument("--emit-fixtures", action="store_true")
    p.add_argument("--out", type=Path)
    p.add_argument("--fixture-dir", type=Path)
    args = p.parse_args()

    if args.emit_fixtures:
        asyncio.run(emit_fixtures(args.fixture_dir))
        return

    if args.out is None:
        p.error("--out is required with --variant")

    asyncio.run(run_one(args.variant, args.out))


if __name__ == "__main__":
    main()
