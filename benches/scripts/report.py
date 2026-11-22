#!/usr/bin/env python3
"""
Collate raw benchmark JSON into a single markdown report.

Input layout (produced by run_all.sh):

    results/<STAMP>/
        env.json
        python/<variant>.json
        node/<variant>.json
        rust-raw/<group>/<case>/new/estimates.json

Output:

    results/<STAMP>/REPORT.md

The report is meant to be read top-to-bottom. It leads with the environment
fingerprint (so a reader can immediately tell whether the numbers apply to
them), then one table per (benchmark, scenario), then the raw JSON filenames
for anyone who wants to dig in.

We deliberately do NOT compute summary numbers ("fury is 1.7x faster than
JSON") in this script. Summaries invite cherry-picking; readers should look
at the tables and draw their own conclusion.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


# ────────────────────────────────────────────────────────────────────────
# Loading
# ────────────────────────────────────────────────────────────────────────

def load_json(path: Path) -> dict[str, Any] | None:
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return None


def collect_python(root: Path) -> list[dict]:
    out = []
    for f in sorted((root / "python").glob("*.json")):
        data = load_json(f)
        if data:
            out.append(data)
    return out


def collect_node(root: Path) -> list[dict]:
    out = []
    for f in sorted((root / "node").glob("*.json")):
        data = load_json(f)
        if data:
            out.append(data)
    return out


def collect_rust(root: Path) -> list[dict]:
    """
    Criterion writes `estimates.json` per (group, case). The relevant fields
    are `median.point_estimate` (ns) and `median.confidence_interval`.

    We flatten the tree into a list of {group, case, median_ns, p95_ns?}.
    Criterion doesn't emit p95 directly; we approximate from the slope
    estimate's standard error if it's available, otherwise we skip p95.
    """
    out = []
    raw = root / "rust-raw"
    if not raw.exists():
        return out
    for f in sorted(raw.rglob("*.json")):
        data = load_json(f)
        if not data:
            continue
        rel = f.relative_to(raw).with_suffix("")
        parts = rel.parts
        # Skip the summary sub-files criterion creates; we want per-case only.
        if len(parts) < 3:
            continue
        group, case = parts[0], parts[1]
        try:
            med_ns = data["median"]["point_estimate"]
            lo = data["median"]["confidence_interval"]["lower_bound"]
            hi = data["median"]["confidence_interval"]["upper_bound"]
        except (KeyError, TypeError):
            continue
        out.append({
            "group": group,
            "case": case,
            "median_ns": med_ns,
            "ci_lo_ns": lo,
            "ci_hi_ns": hi,
        })
    return out


# ────────────────────────────────────────────────────────────────────────
# Formatting
# ────────────────────────────────────────────────────────────────────────

def fmt_ms(ns: float | int | None) -> str:
    if ns is None:
        return "—"
    return f"{ns / 1e6:.2f} ms"


def fmt_mb(bytes_: int | None) -> str:
    if bytes_ is None:
        return "—"
    return f"{bytes_ / 1_000_000:.1f} MB"


def python_table(rows):
    if not rows:
        return "_no data_\n"
    head = (
        "| Variant | median | p95 | min | max | iterations | bytes_out | peak RSS |\n"
        "|---|---:|---:|---:|---:|---:|---:|---:|\n"
    )
    body = ""
    for r in sorted(rows, key=lambda x: x.get("median_ms", 0)):
        peak = r.get("peak_rss_bytes")
        peak_str = fmt_mb(peak) if peak else "—"
        body += (
            f"| `{r['variant']}` "
            f"| {r.get('median_ms', 0):.2f} ms "
            f"| {r.get('p95_ms', 0):.2f} ms "
            f"| {r.get('min_ms', 0):.2f} ms "
            f"| {r.get('max_ms', 0):.2f} ms "
            f"| {r.get('iterations', 0)} "
            f"| {fmt_mb(r.get('bytes_out'))} "
            f"| {peak_str} |\n"
        )
    return head + body


def node_table(rows: list[dict]) -> str:
    if not rows:
        return "_no data_\n"
    head = (
        "| Variant | rows | median | p95 | min | max | iterations | fixture bytes |\n"
        "|---|---:|---:|---:|---:|---:|---:|---:|\n"
    )
    body = ""
    for r in sorted(rows, key=lambda x: x.get("median_ms", 0)):
        body += (
            f"| `{r['variant']}` "
            f"| {r.get('rows', '—')} "
            f"| {r.get('median_ms', 0):.2f} ms "
            f"| {r.get('p95_ms', 0):.2f} ms "
            f"| {r.get('min_ms', 0):.2f} ms "
            f"| {r.get('max_ms', 0):.2f} ms "
            f"| {r.get('iterations', 0)} "
            f"| {r.get('fixture_bytes', '—')} |\n"
        )
    return head + body


def rust_table(rows: list[dict]) -> str:
    if not rows:
        return "_no data_\n"
    head = (
        "| Group | Case | median | 95% CI |\n"
        "|---|---|---:|---:|\n"
    )
    body = ""
    for r in sorted(rows, key=lambda x: (x["group"], x["case"])):
        body += (
            f"| `{r['group']}` "
            f"| `{r['case']}` "
            f"| {r['median_ns'] / 1e6:.3f} ms "
            f"| [{r['ci_lo_ns'] / 1e6:.3f}, {r['ci_hi_ns'] / 1e6:.3f}] ms |\n"
        )
    return head + body


# ────────────────────────────────────────────────────────────────────────
# Assembly
# ────────────────────────────────────────────────────────────────────────

def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "results/latest")
    if not root.is_dir():
        print(f"report.py: {root} is not a directory", file=sys.stderr)
        sys.exit(1)

    env = load_json(root / "env.json") or {}
    py = collect_python(root)
    nd = collect_node(root)
    rs = collect_rust(root)

    out = [f"# FuRy benchmark report\n", f"Run: `{root.name}`\n"]

    host = env.get("host", {})
    tc = env.get("toolchain", {})
    out.append("## Environment\n")
    out.append(f"- **CPU:** {host.get('cpu', '—')} ({host.get('arch', '—')})")
    out.append(f"- **RAM:** {host.get('ram_gib', '—')} GiB")
    out.append(f"- **OS:** {host.get('os', '—')}")
    out.append("")
    out.append(f"- **Python:** {tc.get('python', '—')}")
    out.append(f"- **Node:** {tc.get('node', '—')}")
    out.append(f"- **Rust:** {tc.get('rust', '—')}")
    out.append(f"- **Postgres:** {tc.get('postgres', '—')}")
    out.append("")

    out.append("## Python — DB → in-memory → bytes\n")
    out.append("All variants query the same 10 000-row `users` table with the same SQL.")
    out.append("`_to_dicts`/`_to_models` stop at the in-memory representation;")
    out.append("`_to_bytes`/`_json_*`/`_msgpack` also produce wire bytes.")
    out.append("")
    out.append(python_table(py))

    out.append("\n## Node frontend page\n")
    corpora = {row.get("corpus_sha256") for row in nd}
    if nd and (len(corpora) != 1 or None in corpora or len(nd) != 9):
        out.append("Incomplete or mixed fixture data. Re-run all nine variants before comparison.")
    out.append("Each variant opens the same ordered rows. `*_read3` reads three fields per row.")
    out.append("`*_roundtrip` changes one field in each row and emits an updated batch.")
    out.append("")
    out.append(node_table(nd))

    out.append("\n## Rust — codec in isolation\n")
    out.append("Criterion, no Python or JS in the loop. 95% CI is criterion's own.")
    out.append("")
    out.append(rust_table(rs))

    out.append("\n## Raw data\n")
    out.append("Everything below is written by `run_all.sh`. Each file is the raw")
    out.append("measurement; nothing in this report is derived from anything else.")
    out.append("")
    for sub in ("python", "node"):
        for f in sorted((root / sub).glob("*.json")):
            out.append(f"- `{f.relative_to(root)}`")
    out.append(f"- `rust/criterion.txt` (criterion stdout)")
    out.append(f"- `rust-raw/**` (criterion machine-readable estimates)")
    out.append("")

    out.append("## Method\n")
    out.append("See `benches/README.md` for the methodology, the rationale for each choice,")
    out.append("and the list of things this benchmark deliberately does not measure.\n")

    (root / "REPORT.md").write_text("\n".join(out))
    print(f"→ wrote {root / 'REPORT.md'}")


if __name__ == "__main__":
    main()
