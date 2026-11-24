#!/usr/bin/env bash
set -euo pipefail

mode=${1:-serve}
case "$mode" in
    serve|smoke|bench|load|profile) ;;
    *) echo "usage: bash examples/quiz/run.sh [serve|smoke|bench|load|profile]" >&2; exit 2 ;;
esac
if [[ "$mode" == serve ]]; then
    unset QUIZ_COMPARE_CODECS
fi
if [[ "$mode" == serve || "$mode" == smoke ]]; then
    unset QUIZ_DATASET QUIZ_PAGE_SIZE
fi
case "${QUIZ_DATASET:-quiz}" in
    mixed|quiz|orders|cities) ;;
    *) echo 'QUIZ_DATASET must be quiz, mixed, cities or orders' >&2; exit 2 ;;
esac

if [[ "${QUIZ_DATASET:-quiz}" == cities ]]; then
    : "${QUIZ_CITIES_ZIP:?set QUIZ_CITIES_ZIP to the GeoNames cities500.zip snapshot}"
    [[ "$QUIZ_CITIES_ZIP" = /* ]] || export QUIZ_CITIES_ZIP="$PWD/$QUIZ_CITIES_ZIP"
fi

quiz=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$quiz/../.." && pwd)
cd "$repo"
if [[ ! -f scripts/check-bindings.sh ]]; then
    echo "This checkout is missing the FuRy binding build scripts. Run the quiz from a checkout that includes FuRy." >&2
    exit 2
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/target}"
[[ "$CARGO_TARGET_DIR" = /* ]] || CARGO_TARGET_DIR="$repo/$CARGO_TARGET_DIR"
bindings="$CARGO_TARGET_DIR/bindings/current"
if [[ "${QUIZ_REUSE_BINDINGS:-0}" != 1 ]]; then
    FURY_WASM_PROFILE=release bash scripts/check-bindings.sh
fi

work=$(mktemp -d /tmp/fury-quiz.XXXXXXXX)
pid=
cleanup() {
    if [[ -n "$pid" ]]; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    if [[ -f "$work/db/postmaster.pid" ]]; then
        pg_ctl -D "$work/db" -m fast -w stop >/dev/null || return 1
    fi
    rm -rf -- "$work"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

export UV_PROJECT_ENVIRONMENT="$work/venv"
uv_options=(--locked --project "$quiz" --python "${FURY_PYTHON:-python3.14}")
if [[ "${QUIZ_COMPARE_CODECS:-0}" == 1 ]]; then
    uv_options+=(--group codecs)
fi
uv sync "${uv_options[@]}"
uv pip install --python "$work/venv/bin/python" --no-deps "$bindings"/wheels/*.whl
export PYTHONDONTWRITEBYTECODE=1
if [[ "${QUIZ_COMPARE_CODECS:-0}" == 1 ]]; then
    mkdir "$work/codecs"
    if [[ "${QUIZ_DATASET:-quiz}" == orders ]]; then
        proto=orders
        decoder=decode_orders.mjs
        encoder=orders_codecs.py
    else
        proto=leaderboard
        decoder=decode.mjs
        encoder=codecs_bench_encode.py
    fi
    cp "$quiz/codecs/"{"$encoder","$decoder","$proto.proto",package.json,package-lock.json} "$work/codecs/"
    npm ci --prefix "$work/codecs" --ignore-scripts --no-audit --no-fund
    "$work/venv/bin/python" -m grpc_tools.protoc -I "$work/codecs" \
        --python_out="$work/codecs" "$work/codecs/$proto.proto"
    export PYTHONPATH="$work/codecs${PYTHONPATH:+:$PYTHONPATH}"
    export QUIZ_CODEC_DECODER="$work/codecs/$decoder"
fi

cd "$work"
FURY_PYTHON="$work/venv/bin/python" "$CARGO_TARGET_DIR/debug/fury" generate \
    -f python -t ts -s "$quiz/models.py" -d models
cp "$quiz/tsconfig.json" .
mkdir site
cp "$quiz"/frontend/* site/
ln -s "$bindings/web" wasm
cp -R "$bindings/web" site/wasm
node "$bindings/tooling/node_modules/typescript/bin/tsc" --project tsconfig.json
cp models/.fury_registry.bin site/registry.bin
printf '{"type":"module"}\n' > site/package.json
mkdir -p site/node_modules/@fury
ln -s "$work/site/wasm" site/node_modules/@fury/wasm-core

initdb -D "$work/db" -A trust -U fury --locale=C --encoding=UTF8 >/dev/null
pg_ctl -D "$work/db" -l "$work/pg.log" -o "-k $work -h '' -F" -w start >/dev/null
export QUIZ_DATABASE_URL="postgresql://fury@localhost/postgres?host=$work"
export QUIZ_PG_SOCKET="$work"
export QUIZ_SITE="$work/site" QUIZ_SEED=1
if [[ "$mode" == serve ]]; then
    port=${QUIZ_PORT:-8001}
else
    port=$("$work/venv/bin/python" -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
fi
origin="http://127.0.0.1:$port"
uv run --project "$quiz" --no-sync python -m uvicorn app:app --app-dir "$quiz" \
    --host 127.0.0.1 --port "$port" --no-access-log &
pid=$!
for ((attempt=0; attempt<100; attempt++)); do
    if curl -fsS "$origin/api/questions" >/dev/null 2>&1; then break; fi
    kill -0 "$pid"
    sleep 0.1
done
curl -fsS "$origin/api/questions" >/dev/null
case "$mode" in
    serve)
        printf 'Open %s\n' "$origin"
        wait "$pid"
        ;;
    smoke)
        "$work/venv/bin/python" "$quiz/smoke.py" "$origin"
        node "$quiz/compat.mjs" "$QUIZ_SITE"
        node "$quiz/bench.mjs" "$origin" "$QUIZ_SITE" 3
        ;;
    bench) node "$quiz/bench.mjs" "$origin" "$QUIZ_SITE" "${QUIZ_BENCH_ITERATIONS:-50}" ;;
    profile)
        "$work/venv/bin/python" "$quiz/backend_profile.py"
        QUIZ_PROFILE=1 node "$quiz/bench.mjs" "$origin" "$QUIZ_SITE" "${QUIZ_BENCH_ITERATIONS:-50}"
        ;;
    load) node "$quiz/bench.mjs" "$origin" "$QUIZ_SITE" load "${QUIZ_LOAD_SECONDS:-5}" ;;
esac
