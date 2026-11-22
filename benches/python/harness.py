"""
Minimal timing harness.

Design notes, in order of how often someone asks:

  1. Why not pyperf?  pyperf is the right tool for CPU-bound microbenchmarks
     where you control the entire run loop. Here the run loop is async, hits
     a database, and has warmup semantics (prepared statements, page cache)
     that pyperf's model doesn't capture well. Rolling our own means every
     decision is visible in this file.

  2. Why warmup?  Three reasons. (a) Postgres caches the query plan and pages
     after the first execution. (b) CPython type-caches and inline caches warm
     on repeated calls. (c) The OS page cache warms the binary. None of these
     are what we want to measure.

  3. Why median and not mean?  Wall-clock time is right-skewed. A single
     scheduler preemption can double the mean while leaving the median alone.
     The user experiences the median, not the mean.

  4. Why gc.collect() before each measured iteration?  So the number reflects
     the scenario, not the cost of cleaning up the previous iteration. We
     collect outside the timed region so the collection itself isn't measured.

  5. Why ru_maxrss?  It's the only memory number that captures both the
     Python heap and the Rust allocations done by fury-core via PyO3. Note the
     unit differs by platform: bytes on macOS, KB on Linux. We normalize.
"""

from __future__ import annotations

import gc
import platform
import resource
import statistics
import time
from dataclasses import dataclass, field
from typing import Awaitable, Callable


def _peak_rss_bytes() -> int:
    """Peak RSS in bytes, normalized across platforms."""
    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if platform.system() == "Darwin":
        return int(rss)          # macOS reports bytes
    return int(rss) * 1024       # Linux reports KiB


@dataclass
class Result:
    variant: str
    times_ns: list[int] = field(default_factory=list)
    bytes_out: int | None = None
    fixture_bytes: int | None = None
    extra: dict = field(default_factory=dict)

    @property
    def median_ms(self) -> float:
        return statistics.median(self.times_ns) / 1e6

    @property
    def p95_ms(self) -> float:
        s = sorted(self.times_ns)
        # Nearest-rank p95. With n >= 30 this is stable enough.
        return s[min(len(s) - 1, int(len(s) * 0.95))] / 1e6

    @property
    def min_ms(self) -> float:
        return min(self.times_ns) / 1e6

    @property
    def max_ms(self) -> float:
        return max(self.times_ns) / 1e6

    def as_dict(self) -> dict:
        return {
            "variant": self.variant,
            "median_ms": round(self.median_ms, 4),
            "p95_ms": round(self.p95_ms, 4),
            "min_ms": round(self.min_ms, 4),
            "max_ms": round(self.max_ms, 4),
            "iterations": len(self.times_ns),
            "bytes_out": self.bytes_out,
            "fixture_bytes": self.fixture_bytes,
            "peak_rss_bytes": _peak_rss_bytes(),
            **self.extra,
        }


async def run_bench(
    variant: str,
    fn: Callable[[], Awaitable[object]],
    *,
    warmup: int = 5,
    iterations: int = 30,
) -> Result:
    """
    Run `fn` `warmup` times, discard, then `iterations` times and collect
    wall-clock nanoseconds around each call.

    `fn` must be a zero-argument async callable. Anything the scenario needs
    (connection, query, schema) should be closed over. This keeps the timed
    region free of setup code that would otherwise show up in every variant.
    """
    for _ in range(warmup):
        await fn()

    times: list[int] = []
    bytes_out: int | None = None

    for _ in range(iterations):
        gc.collect()  # outside the timed region
        t0 = time.perf_counter_ns()
        out = await fn()
        t1 = time.perf_counter_ns()
        times.append(t1 - t0)
        if out is not None:
            if isinstance(out, (bytes, bytearray)):
                bytes_out = len(out)
            elif hasattr(out, "byte_len"):
                bytes_out = out.byte_len

    result = Result(variant=variant, times_ns=times, bytes_out=bytes_out)

    spread = (result.p95_ms - result.median_ms) / result.median_ms
    if spread > 0.5:
        print(
            f"  ! {variant}: p95/median spread = {spread:.0%}. "
            f"Measurement is noisy; consider more iterations or a quieter machine."
        )
    return result