"""Check packaged Python and WASM, TypeScript output and shared wire frames."""

import argparse
import asyncio
from datetime import datetime, timezone
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys

import fury


HERE = Path(__file__).resolve().parent
MODELS = HERE / "interop_models.py"


def generate(cli, cwd):
    cwd.mkdir()
    (cwd / MODELS.name).write_text("raise RuntimeError('imported the wrong models')\n")
    subprocess.run(
        [cli, "generate", "-f", "python", "-t", "ts", "-s", MODELS, "-d", "out"],
        cwd=cwd, env={**os.environ, "FURY_PYTHON": sys.executable}, check=True,
    )
    return {path.name: path.read_bytes() for path in (cwd / "out").iterdir()}


def fields(n, text, active):
    encoded = text.encode("utf-8")
    return {
        "id": b"\x01" + struct.pack("<q", n),
        "text": b"\x01" + struct.pack("<I", len(encoded)) + encoded,
        "active": b"\x01" + bytes([int(active)]),
        "note": b"\x00",
    }


def encode_frame(entries):
    body = bytearray()
    for name, value in entries.items():
        name = name.encode("ascii")
        body.extend(bytes([len(name)]) + name + struct.pack("<I", len(value)) + value)
    return struct.pack("<I", len(body) + 6) + b"F\x02" + body


def decode_fields(frame):
    assert len(frame) == struct.unpack_from("<I", frame)[0] and frame[4:6] == b"F\x02"
    entries = {}
    pos = 6
    while pos < len(frame):
        n = frame[pos]
        name = frame[pos + 1 : pos + 1 + n].decode("utf-8")
        pos += n + 1
        length = struct.unpack_from("<I", frame, pos)[0]
        pos += 4
        assert length and pos + length <= len(frame) and name not in entries
        entries[name] = frame[pos : pos + length]
        pos += length
    return entries


def unsigned_registry():
    def name(s):
        return struct.pack("<Q", len(s)) + s.encode("ascii")

    # FieldType discriminants for UInt64 and UInt8 in the fixed-width cache.
    return b"".join([
        struct.pack("<Q", 1), name("Unsigned"), struct.pack("<Q", 2),
        name("total"), struct.pack("<IB", 7, 0), name("count"), struct.pack("<IB", 4, 0),
    ])


async def select_message(model):
    db = fury.FuryDB.connect(os.environ["FURY_TEST_DATABASE_URL"])
    row = await db.fetch_one(
        model, "SELECT $1::bigint AS id, $2::text AS text, true AS active, NULL::text AS note",
        [2**53 + 1, "Café"],
    )
    assert (row.id, row.text, row.active, row.note) == (2**53 + 1, "Café", True, None)
    try:
        await db.fetch_one(model, "SELECT $1::timestamp", [datetime(2026, 11, 20, tzinfo=timezone.utc)])
    except ValueError as error:
        assert "timezone-naive" in str(error)
    else:
        raise AssertionError("SQL binding lost timezone information")
    return row.to_bytes()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("cli", type=Path)
    parser.add_argument("work", type=Path)
    parser.add_argument("--postgres", action="store_true")
    args = parser.parse_args()
    assert Path(fury.__file__).resolve().is_relative_to(Path(sys.prefix).resolve())
    run = args.work / "consumer"
    outputs = generate(args.cli, run)
    assert set(outputs) == {"message.ts", "summary.ts", ".fury_registry.bin"}
    assert outputs == generate(args.cli, args.work / "repeat"), "code or registry differs between builds"

    survivor = run / "survivor.py"
    survivor.write_text("from fury import BaseModel\nclass Survivor(BaseModel):\n    value: int\n")
    subprocess.run([sys.executable, "-I", survivor], cwd=run, check=True)
    cache = run / ".fury_registry.bin"
    before = cache.read_bytes()
    subprocess.run(
        [args.cli, "generate", "-f", "python", "-t", "ts", "-s", MODELS, "-d", "."],
        cwd=run, env={**os.environ, "FURY_PYTHON": sys.executable}, check=True,
    )
    assert cache.read_bytes() == before, "in-place generation lost unrelated schemas"

    failed = args.work / "unwritable-cache"
    failed.mkdir()
    (failed / ".fury_registry.bin").mkdir()
    failure = subprocess.run(
        [args.cli, "generate", "-f", "python", "-t", "ts", "-s", MODELS, "-d", "out"],
        cwd=failed, env={**os.environ, "FURY_PYTHON": sys.executable},
        capture_output=True, text=True,
    )
    assert failure.returncode != 0 and "save registry" in failure.stderr
    assert not (failed / "out").exists()

    os.chdir(run)
    spec = importlib.util.spec_from_file_location("fixture_models", MODELS)
    models = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = models
    spec.loader.exec_module(models)
    try:
        class Ambiguous(fury.BaseModel):
            payload: int | str
    except TypeError:
        pass
    else:
        raise AssertionError("ambiguous union silently chose one wire type")
    try:
        type("A" * 256, (fury.BaseModel,), {"__annotations__": {"id": int}})
    except ValueError:
        pass
    else:
        raise AssertionError("registry accepted a model name beyond the cache limit")

    expected = fields(2**53 + 1, "Café", True)
    frame = asyncio.run(select_message(models.Message)) if args.postgres else encode_frame(expected)
    assert decode_fields(frame) == expected
    Path("message.bin").write_bytes(frame)
    Path("unsigned.registry").write_bytes(unsigned_registry())
    shutil.copy(HERE.parents[1] / "fury-core/tests/fixtures/v2-id42.fury", "golden.fury")
    shutil.copy(HERE / "consumer.ts", "consumer.ts")
    shutil.copy(HERE / "interop.cjs", "interop.cjs")

    # Install the same npm tarball a consumer would receive.
    (run / "package.json").write_text('{"name":"fury-consumer","version":"0.0.0","private":true}\n')
    pack = subprocess.check_output(
        ["npm", "pack", "--ignore-scripts", "--json", args.work / "wasm"], text=True,
    )
    tarball = json.loads(pack)[0]["filename"]
    subprocess.run(["npm", "install", "--no-audit", "--no-fund", "--ignore-scripts",
                    f"@fury/wasm-core@file:{tarball}"], check=True)
    compiler = args.work / "tooling/node_modules/typescript/bin/tsc"
    subprocess.run(["node", compiler, "--strict", "--target", "ES2022", "--module", "commonjs",
                    "--lib", "ES2022,DOM,ESNext.Disposable", "--outDir", "js", "consumer.ts"], check=True)
    subprocess.run(["node", "interop.cjs"], check=True)
    subprocess.run(["node", HERE / "interop_web.mjs", args.work / "web"], check=True)
    assert decode_fields(Path("updated.bin").read_bytes()) == fields(-(2**53 + 1), "updated ✓", False)
    print("Binding conformance passed" + (" with PostgreSQL" if args.postgres else ""))


if __name__ == "__main__":
    main()
