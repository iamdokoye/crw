#!/usr/bin/env bash
# Verify the MCP registry has io.github.us/crw at the expected version.
#
# Usage: verify_mcp_registry.sh <version> [server_name=io.github.us/crw]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

v="${1:?version required}"
name="${2:-io.github.us/crw}"

end=$((SECONDS + 300))
while (( SECONDS < end )); do
  if curl -fsSL "https://registry.modelcontextprotocol.io/v0/servers" 2>/dev/null \
      | jq -e --arg n "$name" --arg v "$v" \
        'any(.servers[]?; .name == $n and .version == $v)' >/dev/null 2>&1; then
    notice "mcp-registry $name@$v present"
    exit 0
  fi
  sleep 10
done
die "mcp-registry $name@$v not visible after 5min"
