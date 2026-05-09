#!/usr/bin/env bash
# Verify the crw PyPI package has the expected version.
#
# Usage: verify_pypi.sh <version>
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

v="${1:?version required}"
end=$((SECONDS + 300))
while (( SECONDS < end )); do
  if curl -fsSL "https://pypi.org/pypi/crw/${v}/json" 2>/dev/null \
      | jq -e --arg v "$v" '.info.version == $v' >/dev/null 2>&1; then
    notice "pypi crw@$v present"
    exit 0
  fi
  sleep 10
done
die "pypi crw@$v not visible after 5min"
