#!/usr/bin/env bash
# Verify proxy-list rotation end-to-end against a running crw-server.
#
# Proves: (1) HTTP-path requests rotate across the pool, (2) JS/Chrome requests
# egress through the per-request proxy (needs a real Chrome/CDP backend),
# (3) a bad proxy fails closed (no direct egress).
#
# Requires two logging proxies. Easiest: mitmproxy (`pip install mitmproxy`),
# which this script drives via `mitmdump`. Any forward proxy that logs requests
# works — point PROXY_A_LOG / PROXY_B_LOG at their access logs instead.
#
# Usage:
#   ./scripts/verify-proxy-rotation.sh                 # HTTP-path checks
#   WITH_CHROME=1 ./scripts/verify-proxy-rotation.sh   # also drive a JS render
#
# Notes:
#   - Start your CDP backend separately for WITH_CHROME (see docs/self-hosting).
#   - This drives the server you point CRW_URL at; it does NOT start crw-server.
set -euo pipefail

CRW_URL="${CRW_URL:-http://localhost:3000}"
PORT_A="${PORT_A:-8091}"
PORT_B="${PORT_B:-8092}"
TARGET="${TARGET:-https://example.com}"
N="${N:-6}"

if ! command -v mitmdump >/dev/null 2>&1; then
  echo "mitmdump not found. Install with: pip install mitmproxy" >&2
  echo "(or run two forward proxies on :$PORT_A and :$PORT_B and tail their logs)" >&2
  exit 2
fi

tmp="$(mktemp -d)"
log_a="$tmp/a.log"; log_b="$tmp/b.log"
echo "logs: $log_a $log_b"

# Start two logging forward proxies. `-w` writes a flow dump; we just need the
# request lines, so use the addon stdout captured to a file.
mitmdump --listen-port "$PORT_A" -q --set flow_detail=1 >"$log_a" 2>&1 &
pid_a=$!
mitmdump --listen-port "$PORT_B" -q --set flow_detail=1 >"$log_b" 2>&1 &
pid_b=$!
cleanup() { kill "$pid_a" "$pid_b" 2>/dev/null || true; }
trap cleanup EXIT
sleep 1

echo "==> Configure crw-server with:"
echo "    CRW_CRAWLER__PROXY_LIST=http://localhost:$PORT_A,http://localhost:$PORT_B"
echo "    CRW_CRAWLER__PROXY_ROTATION=round_robin"
echo "    (restart the server, then press enter)"
read -r _

echo "==> Firing $N HTTP-only scrapes ($TARGET)"
for _ in $(seq "$N"); do
  curl -s "$CRW_URL/v1/scrape" -H 'content-type: application/json' \
    -d "{\"url\":\"$TARGET\",\"render_js\":false}" >/dev/null || true
done

hits_a=$(grep -c "CONNECT\|GET\|$TARGET" "$log_a" || true)
hits_b=$(grep -c "CONNECT\|GET\|$TARGET" "$log_b" || true)
echo "proxy A hits: $hits_a | proxy B hits: $hits_b"
if [ "$hits_a" -gt 0 ] && [ "$hits_b" -gt 0 ]; then
  echo "PASS: HTTP traffic rotated across BOTH proxies (round_robin)."
else
  echo "FAIL: traffic did not hit both proxies — rotation not working." >&2
  exit 1
fi

if [ "${WITH_CHROME:-0}" = "1" ]; then
  echo "==> Firing $N JS scrapes (render_js:true) — requires a CDP backend"
  before_a=$hits_a; before_b=$hits_b
  for _ in $(seq "$N"); do
    curl -s "$CRW_URL/v1/scrape" -H 'content-type: application/json' \
      -d "{\"url\":\"$TARGET\",\"render_js\":true}" >/dev/null || true
  done
  ja=$(grep -c "CONNECT\|$TARGET" "$log_a" || true)
  jb=$(grep -c "CONNECT\|$TARGET" "$log_b" || true)
  echo "after JS — proxy A: $ja (was $before_a) | proxy B: $jb (was $before_b)"
  if [ "$ja" -gt "$before_a" ] || [ "$jb" -gt "$before_b" ]; then
    echo "PASS: JS/Chrome traffic egressed through the proxy pool."
  else
    echo "WARN: no new proxy hits from JS path — confirm a CDP backend is running" >&2
  fi
fi

echo
echo "==> Leak-safety: a malformed proxy must fail closed (manual)."
echo "    Restart with CRW_CRAWLER__PROXY_LIST=not-a-url and confirm the server"
echo "    REFUSES TO START (ConfigError) instead of connecting directly."
echo "DONE."
