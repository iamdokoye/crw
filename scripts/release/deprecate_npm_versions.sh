#!/usr/bin/env bash
# Mark every npm package@version as deprecated. Used to retire 0.6.0 after
# the optionalDependencies pin regression (codex review v4 C9): the package
# is published with broken pins and npm versions are effectively immutable,
# so we deprecate and ship a corrected version.
#
# Idempotent: skips packages that don't exist at $version.
#
# Usage: deprecate_npm_versions.sh <version> [reason]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

v="${1:?version required}"
reason="${2:-Use a newer version}"

pkgs=(crw-mcp crw-mcp-darwin-x64 crw-mcp-darwin-arm64
      crw-mcp-linux-x64 crw-mcp-linux-arm64
      crw-mcp-win32-x64 crw-mcp-win32-arm64)

for p in "${pkgs[@]}"; do
  exists=$(npm view "$p@$v" version 2>/dev/null || true)
  if [ -z "$exists" ]; then
    notice "skip $p@$v (not on registry)"
    continue
  fi
  npm deprecate "$p@$v" "$reason"
  notice "deprecated $p@$v"
done
