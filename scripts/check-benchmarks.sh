#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

repo=$PWD
target=${CARGO_TARGET_DIR:-$repo/target}
[[ "$target" = /* ]] || target="$repo/$target"
export FURY_BENCH_PYTHON="$target/bindings/current/venv/bin/python"
export FURY_BENCH_WASM="$target/bindings/current/wasm/fury_ts.js"
npm ci --prefix benches/node --ignore-scripts --no-audit --no-fund
node --test benches/node/benchmark.test.mjs
node --expose-gc benches/node/check.mjs --smoke

if [[ -n "${BASE_SHA:-}" ]]; then
    git cat-file -e "$BASE_SHA^{commit}"
    if git cat-file -e "$BASE_SHA:fury-ts/Cargo.toml" 2>/dev/null; then
        base=$(mktemp -d "$target/bench-base.XXXXXXXX")
        git archive "$BASE_SHA" | tar -xf - -C "$base"
        build=("$base/fury-ts" --target nodejs --out-dir "$base/wasm" --release)
        if [[ -f "$base/Cargo.lock" ]]; then build+=(-- --locked); fi
        CARGO_TARGET_DIR="$base/target" wasm-pack build "${build[@]}"
        node --expose-gc benches/node/check.mjs --out "$target/benchmark-results" \
            --baseline-wasm "$base/wasm/fury_ts.js"
        exit
    fi
    echo "The PR base predates the WASM codec. This run establishes the baseline."
fi
case "${GITHUB_EVENT_NAME:-}" in
    schedule|workflow_dispatch)
        node --expose-gc benches/node/check.mjs --out "$target/benchmark-results"
        ;;
esac
