#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

cluster=$(mktemp -d /tmp/fury-check-tls.XXXXXXXX)
cleanup() {
    if [[ -f "$cluster/data/postmaster.pid" ]]; then
        pg_ctl -D "$cluster/data" -m fast -w stop >/dev/null || return 1
    fi
    rm -rf -- "$cluster"
}
trap cleanup EXIT
umask 077

for name in root other; do
    openssl req -new -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=FuRy test CA $name" \
        -keyout "$cluster/$name.key" -out "$cluster/$name.crt" >/dev/null 2>&1
done
openssl req -new -newkey rsa:2048 -nodes -subj /CN=localhost \
    -keyout "$cluster/server.key" -out "$cluster/server.csr" >/dev/null 2>&1
cat > "$cluster/server.ext" <<'CONF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:localhost
CONF
openssl x509 -req -sha256 -days 1 -in "$cluster/server.csr" -extfile "$cluster/server.ext" \
    -CA "$cluster/root.crt" -CAkey "$cluster/root.key" -CAcreateserial -out "$cluster/server.crt" >/dev/null 2>&1

port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
initdb -D "$cluster/data" -A trust -U fury >/dev/null
printf 'local all all trust\nhostssl all all 127.0.0.1/32 trust\n' > "$cluster/data/pg_hba.conf"
pg_ctl -D "$cluster/data" -l "$cluster/server.log" -w start \
    -o "-k $cluster -h 127.0.0.1 -p $port -c ssl=on -c ssl_cert_file=$cluster/server.crt -c ssl_key_file=$cluster/server.key"
export FURY_TEST_TLS_PORT="$port" FURY_TEST_TLS_DIRECTORY="$cluster"
cargo test --locked -p fury-core --test postgres_tls -- --ignored
