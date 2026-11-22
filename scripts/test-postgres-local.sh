#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-rust}
if (( $# > 0 )); then shift; fi
case "$mode" in
    rust|bindings|web|web-serve) ;;
    *) echo "usage: bash scripts/test-postgres-local.sh [rust|bindings|web|web-serve]" >&2; exit 2 ;;
esac
if [[ "$mode" != web && $# -gt 0 ]]; then
    echo "only the web mode accepts additional smoke-test arguments" >&2
    exit 2
fi

cluster=$(mktemp -d /tmp/fury-check-pg.XXXXXXXX)
cleanup() {
    if [[ -f "$cluster/data/postmaster.pid" ]]; then
        pg_ctl -D "$cluster/data" -m fast -w stop >/dev/null || return 1
    fi
    rm -rf -- "$cluster"
}
trap cleanup EXIT
initdb -D "$cluster/data" -A trust -U fury >/dev/null
pg_ctl -D "$cluster/data" -l "$cluster/server.log" -o "-k $cluster -h '' -F" -w start
export FURY_TEST_DATABASE_URL="postgresql://fury@localhost/postgres?host=$cluster"
case "$mode" in
    rust) cargo test --locked -p fury-core --test postgres_contract -- --ignored ;;
    bindings) bash scripts/check-bindings.sh --postgres ;;
    web|web-serve)
        bash scripts/check-bindings.sh --postgres
        if [[ "$mode" == web-serve ]]; then
            bash scripts/check-web-demo.sh --serve
        else
            bash scripts/check-web-demo.sh "$@"
        fi
        ;;
esac
