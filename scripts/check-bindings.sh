#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
    "") ;;
    --postgres) : "${FURY_TEST_DATABASE_URL:?set the URL of a disposable PostgreSQL database}" ;;
    *) echo "usage: bash scripts/check-bindings.sh [--postgres]" >&2; exit 2 ;;
esac
repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/target}"
[[ "$CARGO_TARGET_DIR" = /* ]] || CARGO_TARGET_DIR="$repo/$CARGO_TARGET_DIR"
unset PYTHONPATH
export PYTHONNOUSERSITE=1 PYTHONDONTWRITEBYTECODE=1
python_bin=${FURY_PYTHON:-python3}
case "${FURY_WASM_PROFILE:-dev}" in
    dev|release) ;;
    *) echo 'FURY_WASM_PROFILE must be dev or release' >&2; exit 2 ;;
esac

mkdir -p "$CARGO_TARGET_DIR/bindings"
work=$(mktemp -d "$CARGO_TARGET_DIR/bindings/build.XXXXXXXX")
if [[ "$(uname -s)" == Darwin ]]; then
    # Preserve Mach-O alignment for the packaged native library.
    export CARGO_PROFILE_RELEASE_STRIP=none
fi
maturin build --locked --release --strip=false --features extension-module \
    --manifest-path fury-py/Cargo.toml --interpreter "$python_bin" \
    --compatibility linux --out "$work/wheels"
"$python_bin" -m venv "$work/venv"
wheel=("$work"/wheels/*.whl)
[[ ${#wheel[@]} == 1 && -f "${wheel[0]}" ]]
"$work/venv/bin/python" -m pip install --disable-pip-version-check --no-index --no-deps "${wheel[0]}"

cargo build --locked -p fury-cli
wasm-pack build fury-ts --target nodejs --out-dir "$work/wasm" "--${FURY_WASM_PROFILE:-dev}" -- --locked
wasm-pack build fury-ts --target web --out-dir "$work/web" "--${FURY_WASM_PROFILE:-dev}" -- --locked
mkdir "$work/tooling"
cp fury-cli/tests/typescript/package{,-lock}.json "$work/tooling/"
npm ci --prefix "$work/tooling" --ignore-scripts --no-audit --no-fund
"$work/venv/bin/python" -I fury-cli/tests/check_bindings.py \
    "$CARGO_TARGET_DIR/debug/fury" "$work" "$@"

# Publish one verified wheel/WASM pair to subsequent benchmark steps.
"$python_bin" - "$work" <<'PY'
from pathlib import Path
import sys
work = Path(sys.argv[1])
link = work / "verified"
link.symlink_to(work.name)
link.replace(work.parent / "current")
PY
