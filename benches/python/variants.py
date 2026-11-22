"""
The variants under test. One function per variant, one scenario per function.

The SQL is defined once at module level so `python/` and `node/` (via the
fixture generator) use byte-identical query text. If you change it here, the
fixtures will be regenerated on the next run; that's intentional.

What's in the comparison:

  asyncpg_to_dicts          — the floor. Psycopg/asyncpg returning dicts.
                              Nothing else done. This is what a lot of people
                              ship when "we just need the data".

  asyncpg_to_json_stdlib    — dicts + stdlib json.dumps, with a sensible
                              default= for datetime/UUID/bytes (base64 for
                              bytes, isoformat for datetime). The honest
                              "we use the standard library" baseline.

  asyncpg_to_json_orjson    — dicts + orjson.dumps. Same defaults. The
                              "we use the fast library" baseline. This is
                              the number to beat.

  asyncpg_to_msgpack        — dicts + msgpack.packb. A binary baseline that
                              (unlike JSON) can store bytes natively, so its
                              payload will be smaller.

  fury_to_models            — fury's DB driver, stop at the model. Isolates
                              the row → buffer path from the buffer → bytes
                              path.

  fury_to_bytes             — fury's full path: DB → model → bytes. The
                              number that goes into a wire payload.

  fury_to_models_read3      — fury's DB driver + read 3 fields. This is the
                              scenario that matches a "list users" UI.
"""


import base64
import json
from datetime import datetime
from typing import Any, Callable, Coroutine
from uuid import UUID

import asyncpg
import msgpack
import orjson

# ────────────────────────────────────────────────────────────────────────
# The query. One source of truth.
# ────────────────────────────────────────────────────────────────────────

# Wide query: pulls all 12 columns, including the two fat ones.
# The "read 3 fields" scenario reads id/username/is_active from the result.
WIDE_QUERY = """
    SELECT id, username, email, display_name, is_active, reputation,
           flags, external_id, created_at, updated_at, bio, avatar
    FROM users
    ORDER BY id
    LIMIT $1
"""

# Narrow query: pulls only the three fields we'd render in a list view.
NARROW_QUERY = """
    SELECT id, username, is_active
    FROM users
    ORDER BY id
    LIMIT $1
"""


# ────────────────────────────────────────────────────────────────────────
# JSON defaults. Base64 for bytes, isoformat for datetimes, str for UUIDs.
# These are what people actually do; not what would be technically optimal.
# ────────────────────────────────────────────────────────────────────────

def _json_default(o: Any):
    if isinstance(o, datetime):
        return o.isoformat()
    if isinstance(o, UUID):
        return str(o)
    if isinstance(o, (bytes, bytearray, memoryview)):
        return base64.b64encode(bytes(o)).decode("ascii")
    raise TypeError(f"not serializable: {type(o)}")


def _msgpack_default(o: Any):
    # msgpack has native bytes; it only needs a default for datetime and UUID.
    if isinstance(o, datetime):
        return o.isoformat()
    if isinstance(o, UUID):
        return str(o)
    raise TypeError(f"not serializable: {type(o)}")


# ────────────────────────────────────────────────────────────────────────
# Variant factories.
#
# Each factory returns (variant_name, async_callable). The callable closes
# over whatever connection it needs and is the ONLY thing timed.
#
# Note on connection lifecycle: each factory opens its own pool and closes it
# on cleanup. run.py calls the cleanup after the benchmark completes. We do
# not share connections across variants; that would leak prepared-statement
# state and cache warmth from one into the next.
# ────────────────────────────────────────────────────────────────────────

VariantCallable = Callable[[], Coroutine[Any, Any, Any]]
Cleanup = Callable[[], Coroutine[Any, Any, None]]


async def make_asyncpg_variant(
    dsn: str,
    query: str,
    n: int,
    transform: Callable[[list[dict]], Any],
    name: str,
) -> tuple[str, VariantCallable, Cleanup]:
    """
    Открывает соединение здесь и сейчас — до возврата из фабрики. Так
    run.py не должен ничего знать про setup/teardown, а warmup начинается
    уже с живым соединением.
    """
    conn = await asyncpg.connect(dsn)

    async def timed():
        rows = await conn.fetch(query, n)
        dicts = [dict(r) for r in rows]
        return transform(dicts)

    async def cleanup():
        await conn.close()

    return name, timed, cleanup


async def make_fury_variant(
    dsn: str, n: int, mode: str,
) -> tuple[str, VariantCallable, Cleanup]:
    from fury import FuryDB, BaseModel

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

    # FuryDB.connect синхронный — внутри block_on(tokio), await не нужен.
    db = FuryDB.connect(dsn)

    async def timed():
        models = await db.fetch_all(BenchUser, WIDE_QUERY, [n])
        if mode == "models":
            return None
        if mode == "models_read3":
            total = 0
            for m in models:
                total += m.id
                total += len(m.username)
                total += int(m.is_active)
            return total
        if mode == "bytes":
            total = 0
            for m in models:
                total += len(m.to_bytes())
            return total
        raise ValueError(mode)

    async def cleanup():
        pass

    name = {"models": "fury_to_models",
            "bytes": "fury_to_bytes",
            "models_read3": "fury_to_models_read3"}[mode]
    return name, timed, cleanup


async def build(dsn: str, variant: str, n: int = 1000):
    """Диспетчер. Async, потому что фабрики открывают соединения."""
    if variant == "asyncpg_to_dicts":
        return await make_asyncpg_variant(dsn, WIDE_QUERY, n, lambda d: d, variant)
    if variant == "asyncpg_to_json_stdlib":
        return await make_asyncpg_variant(
            dsn, WIDE_QUERY, n,
            lambda d: json.dumps(d, default=_json_default).encode("utf-8"),
            variant,
        )
    if variant == "asyncpg_to_json_orjson":
        return await make_asyncpg_variant(
            dsn, WIDE_QUERY, n,
            lambda d: orjson.dumps(d, default=_json_default),
            variant,
        )
    if variant == "asyncpg_to_msgpack":
        return await make_asyncpg_variant(
            dsn, WIDE_QUERY, n,
            lambda d: msgpack.packb(d, default=_msgpack_default),
            variant,
        )
    if variant == "fury_to_models":
        return await make_fury_variant(dsn, n, "models")
    if variant == "fury_to_bytes":
        return await make_fury_variant(dsn, n, "bytes")
    if variant == "fury_to_models_read3":
        return await make_fury_variant(dsn, n, "models_read3")
    raise ValueError(f"unknown variant: {variant}")


ALL_VARIANTS = [
    "asyncpg_to_dicts",
    "asyncpg_to_json_stdlib",
    "asyncpg_to_json_orjson",
    "asyncpg_to_msgpack",
    "fury_to_models",
    "fury_to_bytes",
    "fury_to_models_read3",
]