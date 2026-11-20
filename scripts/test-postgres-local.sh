#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-rust}
case "$mode" in
    rust|bindings) ;;
    *) echo "usage: bash scripts/test-postgres-local.sh [rust|bindings]" >&2; exit 2 ;;
esac

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
if [[ "$mode" == bindings ]]; then
    bash scripts/check-bindings.sh --postgres
else
    cargo test --locked -p fury-core --test postgres_contract -- --ignored
fi
