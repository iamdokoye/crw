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

# The bare /v0/servers list is paginated (~30 per page, cursor-based), so our
# server is almost never on page 1 — that check could never pass. Use the
# search filter, which returns every version of just this server, and read the
# nested `.server.*` schema fields.
end=$((SECONDS + 180))
while (( SECONDS < end )); do
  if curl -fsSL "https://registry.modelcontextprotocol.io/v0/servers?search=$name" 2>/dev/null \
      | jq -e --arg n "$name" --arg v "$v" \
        'any(.servers[]?; .server.name == $n and .server.version == $v)' >/dev/null 2>&1; then
    notice "mcp-registry $name@$v present"
    exit 0
  fi
  sleep 10
done
die "mcp-registry $name@$v not visible after 3min"
