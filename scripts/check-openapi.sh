#!/usr/bin/env bash
# OpenAPI drift guard.
#
# When a built crw-server binary is available this script:
#   1. Starts the server on a free loopback port.
#   2. Curls GET /openapi.json from the live binary.
#   3. Diffs the response against the committed docs/openapi.json.
#   4. Asserts that docs/openapi.json "info.version" matches the workspace
#      Cargo.toml version so the spec version is never left stale.
#
# SAFE TO RUN LOCALLY — if the binary hasn't been built yet the script exits 0
# with a notice rather than failing the developer's pre-push flow.
#
# Usage: bash scripts/check-openapi.sh
#        CRW_SERVER_BIN=/path/to/crw-server bash scripts/check-openapi.sh

set -euo pipefail

cd "$(dirname "$0")/.."

# ── Binary resolution ────────────────────────────────────────────────────────
#
# Honour explicit override; otherwise fall back to the default release-profile
# output path (what `cargo build --release -p crw-server` produces).
BIN="${CRW_SERVER_BIN:-target/release/crw-server}"

if [ ! -x "$BIN" ]; then
    echo "notice: crw-server binary not found at '${BIN}' — skipping OpenAPI drift check."
    echo "        Build with 'cargo build --release -p crw-server' to enable this check."
    exit 0
fi

# ── Version agreement: docs/openapi.json "info.version" == Cargo.toml ────────
CARGO_VERSION="$(python3 - <<'PY'
import tomllib, sys
from pathlib import Path
ws = tomllib.loads(Path("Cargo.toml").read_text()).get("workspace", {})
v = ws.get("package", {}).get("version")
if not v:
    print("error: [workspace.package].version not found in Cargo.toml", file=sys.stderr)
    sys.exit(2)
print(v)
PY
)"

SPEC_VERSION="$(python3 - <<'PY'
import json, sys
from pathlib import Path
spec_path = Path("docs/openapi.json")
if not spec_path.exists():
    print("error: docs/openapi.json not found", file=sys.stderr)
    sys.exit(2)
spec = json.loads(spec_path.read_text())
v = spec.get("info", {}).get("version")
if not v:
    print('error: docs/openapi.json is missing info.version', file=sys.stderr)
    sys.exit(2)
print(v)
PY
)"

if [ "$SPEC_VERSION" != "$CARGO_VERSION" ]; then
    echo "error: OpenAPI version mismatch"
    echo "       docs/openapi.json info.version = '${SPEC_VERSION}'"
    echo "       Cargo.toml [workspace.package].version = '${CARGO_VERSION}'"
    echo ""
    echo "Fix: update docs/openapi.json (and crates/crw-server/openapi/openapi.json)"
    echo "     so info.version matches the workspace version."
    exit 1
fi
echo "ok: docs/openapi.json info.version (${SPEC_VERSION}) matches Cargo.toml"

# ── Find a free port ─────────────────────────────────────────────────────────
# TOCTOU note: the Python socket is closed before the shell variable is
# assigned, so another process could claim the port in that brief window.
# On isolated CI runners (no concurrent port-hungry processes) this race is
# negligible.  If the port is stolen, crw-server logs "Failed to bind to …"
# and exits with code 1; the health-poll below detects the dead process via
# `kill -0` and surfaces the server log, so the failure is visible rather
# than silent.  No retry of port selection is needed in practice.
find_free_port() {
    python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
}

PORT="$(find_free_port)"
SERVER_PID=""
SERVER_LOG="$(mktemp)"
COMMITTED_NORM=""
LIVE_NORM=""
LIVE_SPEC=""

cleanup() {
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -f "$SERVER_LOG" "$COMMITTED_NORM" "$LIVE_NORM" "$LIVE_SPEC"
}
trap cleanup EXIT

# ── Start the server ─────────────────────────────────────────────────────────
# Minimal environment: override host/port via env vars; disable auth and all
# optional subsystems so no external services are required.
CRW_SERVER__HOST=127.0.0.1 \
CRW_SERVER__PORT="$PORT" \
CRW_AUTH__API_KEYS="" \
CRW_SEARCH__ENABLED=false \
    "$BIN" &>"$SERVER_LOG" &
SERVER_PID=$!

# ── Wait for the server to accept connections ─────────────────────────────────
# Poll /health up to 10 s in 200 ms steps (50 attempts).
HEALTH_URL="http://127.0.0.1:${PORT}/health"
MAX_ATTEMPTS=50
attempt=0
until curl -sf --max-time 1 "$HEALTH_URL" >/dev/null 2>&1; do
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "error: crw-server exited unexpectedly before becoming ready." >&2
        echo "       Server log:" >&2
        cat "$SERVER_LOG" >&2 || true
        exit 1
    fi
    attempt=$((attempt + 1))
    if [ "$attempt" -ge "$MAX_ATTEMPTS" ]; then
        echo "error: crw-server did not become ready within 10 s." >&2
        echo "       Server log:" >&2
        cat "$SERVER_LOG" >&2 || true
        exit 1
    fi
    sleep 0.2
done

# ── Fetch the live spec ───────────────────────────────────────────────────────
LIVE_SPEC="$(mktemp)"
curl -sf --max-time 10 "http://127.0.0.1:${PORT}/openapi.json" -o "$LIVE_SPEC"

# ── Normalise + diff ──────────────────────────────────────────────────────────
# Round-trip through Python's json module to normalise whitespace / key order
# before diffing so cosmetic formatting differences don't produce false positives.
normalise_json() {
    python3 - "$1" <<'PY'
import json, sys
from pathlib import Path
print(json.dumps(json.loads(Path(sys.argv[1]).read_text()), indent=2, sort_keys=True))
PY
}

COMMITTED_NORM="$(mktemp)"
LIVE_NORM="$(mktemp)"
normalise_json docs/openapi.json   >"$COMMITTED_NORM"
normalise_json "$LIVE_SPEC"        >"$LIVE_NORM"
rm -f "$LIVE_SPEC"

if ! diff --unified=5 "$COMMITTED_NORM" "$LIVE_NORM"; then
    echo ""
    echo "error: OpenAPI spec served by the binary diverges from docs/openapi.json."
    echo ""
    echo "The diff above shows: committed (a) vs live binary (b)."
    echo ""
    echo "Fix: regenerate docs/openapi.json (and crates/crw-server/openapi/openapi.json)"
    echo "     so they match what the binary actually serves."
    rm -f "$COMMITTED_NORM" "$LIVE_NORM"
    exit 1
fi

rm -f "$COMMITTED_NORM" "$LIVE_NORM"
echo "ok: live /openapi.json matches docs/openapi.json — no drift detected"
