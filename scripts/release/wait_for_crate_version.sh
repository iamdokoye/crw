#!/usr/bin/env bash
# Poll crates.io until <crate>@<version> appears in the index, or timeout.
# Replaces the old fixed `sleep 30` between tiers.
#
# Usage: wait_for_crate_version.sh <crate> <version> [timeout_seconds=600]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

crate="${1:?crate required}"
version="${2:?version required}"
timeout_s="${3:-600}"

end=$((SECONDS + timeout_s))
while (( SECONDS < end )); do
  if crate_version_present "$crate" "$version"; then
    notice "$crate@$version present on crates.io"
    exit 0
  fi
  sleep 5
done
die "timed out waiting for $crate@$version on crates.io after ${timeout_s}s"
