"""Profile server-side stages against the quiz's disposable PostgreSQL instance."""

import asyncio
import json
import os
from statistics import median
import tracemalloc
from time import perf_counter_ns

import asyncpg
from fury import FuryDB

from app import ORDERS, PAGE_SIZE, RANKING, RESULT_MODEL, ranking_args


async def profile(label, operation, count):
    await operation()
    samples = []
    for _ in range(count):
        start = perf_counter_ns()
        await operation()
        samples.append((perf_counter_ns() - start) / 1e6)
    tracemalloc.start()
    try:
        await operation()
        _, peak = tracemalloc.get_traced_memory()
    finally:
        tracemalloc.stop()
    print(f"{label} | p50 {median(samples):.3f} ms | Python traced peak {peak} bytes")


async def main():
    count = int(os.environ.get("QUIZ_BENCH_ITERATIONS", "50"))
    if not 3 <= count <= 1000:
        raise ValueError("Choose 3–1000 samples")
    dsn = os.environ["QUIZ_DATABASE_URL"]
    db = FuryDB.connect(dsn, max_connections=4)
    async with asyncpg.create_pool(dsn, host=os.environ["QUIZ_PG_SOCKET"], min_size=1, max_size=4) as pool:
        async def fetch():
            return await pool.fetch(RANKING, *ranking_args())

        rows = [dict(row) for row in await fetch()]
        await profile("asyncpg query", fetch, count)
        async def page():
            return await db.fetch_page(RESULT_MODEL, RANKING, ranking_args(), max_rows=PAGE_SIZE)

        await profile("SQLx query + FuRy frames", page, count)

        async def legacy():
            models = await db.fetch_all(RESULT_MODEL, RANKING, ranking_args())
            return b"".join(row.to_bytes() for row in models)

        await profile("SQLx query + Python models + join", legacy, count)
        encoders = {"stdlib JSON": lambda data: json.dumps(data, ensure_ascii=False, separators=(",", ":")).encode()}
        if os.environ.get("QUIZ_COMPARE_CODECS") == "1":
            if ORDERS:
                from orders_codecs import encode_apache_fory, encode_protobuf
            else:
                from codecs_bench_encode import encode_apache_fory, encode_protobuf
            encoders.update(protobuf=encode_protobuf, apache_fory=encode_apache_fory)
        for name, encode in encoders.items():
            async def encode_snapshot():
                return encode(rows)

            await profile(f"{name} encode from dicts", encode_snapshot, count)


if __name__ == "__main__":
    asyncio.run(main())
