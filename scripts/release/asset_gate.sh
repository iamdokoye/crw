#!/usr/bin/env bash
# Verify the GitHub Release for $tag has all 18 expected binary assets
# (3 binaries × 6 platforms). Runs after publish-binaries job, gates the
# downstream registry publishes.
#
# Usage: asset_gate.sh <tag>
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

tag="${1:?tag required (e.g. v0.6.1)}"

expected=()
for plat_arch in darwin-x64 darwin-arm64 linux-x64 linux-arm64; do
  for bin in crw crw-server crw-mcp; do
    expected+=("${bin}-${plat_arch}.tar.gz")
  done
done
for plat_arch in win32-x64 win32-arm64; do
  for bin in crw crw-server crw-mcp; do
    expected+=("${bin}-${plat_arch}.zip")
  done
done

mapfile -t actual < <(gh release view "$tag" --json assets -q '.assets[].name')

fail=0
for e in "${expected[@]}"; do
  if printf '%s\n' "${actual[@]}" | grep -qx "$e"; then
    printf '✓ %s\n' "$e"
  else
    printf '❌ missing: %s\n' "$e"
    fail=1
  fi
done
exit "$fail"
