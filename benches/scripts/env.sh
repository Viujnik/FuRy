#!/usr/bin/env bash
# Captures a machine + toolchain fingerprint. Emits JSON to stdout.
# Called by run_all.sh before any benchmark starts.
#
# We capture CPU model, RAM, OS, and every toolchain version we use. Without
# this, two runs on different machines look comparable when they aren't.
#
# macOS notes:
#   - `sysctl -n machdep.cpu.brand_string` prints "Apple M4 Pro" on Apple silicon.
#   - `sysctl -n hw.memsize` prints bytes.
#   - `sw_vers -productVersion` prints e.g. "15.2".
# On Linux the same fields are read from /proc/cpuinfo, /proc/meminfo, /etc/os-release.

set -euo pipefail

if [[ "$(uname)" == "Darwin" ]]; then
    CPU="$(sysctl -n machdep.cpu.brand_string)"
    RAM_BYTES="$(sysctl -n hw.memsize)"
    RAM_GIB="$(( RAM_BYTES / 1024 / 1024 / 1024 ))"
    OS="macOS $(sw_vers -productVersion) (Darwin $(uname -r))"
else
    CPU="$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs || echo unknown)"
    RAM_KIB="$(grep -m1 MemTotal /proc/meminfo | awk '{print $2}')"
    RAM_GIB="$(( RAM_KIB / 1024 / 1024 ))"
    OS="$(. /etc/os-release && echo "$PRETTY_NAME") ($(uname -r))"
fi

PY="$(python3 --version 2>&1 | awk '{print $2}' || echo none)"
NODE="$(node --version 2>/dev/null || echo none)"
RUST="$(rustc --version 2>/dev/null | awk '{print $2}' || echo none)"
CARGO="$(cargo --version 2>/dev/null | awk '{print $2}' || echo none)"
PG="$(docker exec fury-bench-postgres postgres --version 2>/dev/null | awk '{print $3}' || echo none)"

cat <<EOF
{
  "captured_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "host": {
    "cpu": "${CPU}",
    "ram_gib": ${RAM_GIB},
    "os": "${OS}",
    "arch": "$(uname -m)"
  },
  "toolchain": {
    "python": "${PY}",
    "node": "${NODE}",
    "rust": "${RUST}",
    "cargo": "${CARGO}",
    "postgres": "${PG}"
  }
}
EOF