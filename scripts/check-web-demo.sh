#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "usage: bash scripts/check-web-demo.sh [--serve | --browser CHROME]" >&2
    exit 2
}

case "${1:-}" in
    "") (($# == 0)) || usage ;;
    --serve) (($# == 1)) || usage ;;
    --browser) (($# == 2)) || usage ;;
    *) usage ;;
esac

repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo"
target=${CARGO_TARGET_DIR:-$repo/target}
[[ "$target" = /* ]] || target="$repo/$target"
bindings="$target/bindings/current"
example="$repo/examples/web"
python="$bindings/venv/bin/python"
cli="$target/debug/fury"
tsc="$bindings/tooling/node_modules/typescript/bin/tsc"
: "${FURY_TEST_DATABASE_URL:?set the URL of an explicitly disposable PostgreSQL database}"
if [[ ! -x "$python" || ! -x "$cli" || ! -f "$tsc" || ! -f "$bindings/web/fury_ts_bg.wasm" ]]; then
    echo "Build the bindings first with bash scripts/check-bindings.sh." >&2
    exit 2
fi

unset PYTHONPATH
export PYTHONNOUSERSITE=1 PYTHONDONTWRITEBYTECODE=1
work=$(mktemp -d "$target/web-demo.XXXXXXXX")
trap 'rm -rf -- "$work"' EXIT
cd "$work"

FURY_PYTHON="$python" "$cli" generate -f python -t ts -s "$example/models.py" -d models
cp "$example/app.ts" "$example/records.ts" "$example/tsconfig.json" .
ln -s "$bindings/web" wasm
node "$tsc" --project tsconfig.json
cp "$example/index.html" "$example/smoke.mjs" site/
cp models/.fury_registry.bin site/registry.bin
ln -s "$bindings/web" site/wasm
printf '{"type":"module"}\n' > site/package.json
mkdir -p site/node_modules/@fury
ln -s "$bindings/web" site/node_modules/@fury/wasm-core

if [[ "${1:-}" == --serve ]]; then
    "$python" -B -s "$example/server.py" "$work/site" --port "${FURY_DEMO_PORT:-8000}"
else
    "$python" -B -s "$example/smoke.py" "$work/site" "$@"
fi
