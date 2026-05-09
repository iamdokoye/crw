#!/usr/bin/env bash
# Verify that every published crate has the exact version on crates.io.
# Reads tier list from release_manifest.toml.
#
# Usage: verify_crates.sh <version> [--manifest PATH]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

version="${1:?version required}"; shift || true
manifest="$SCRIPT_DIR/release_manifest.toml"
while [ $# -gt 0 ]; do
  case "$1" in
    --manifest) manifest="$2"; shift 2 ;;
    *) shift ;;
  esac
done

mapfile -t crates < <(awk '
  /^\[\[tiers\]\]/ { inblock=1; next }
  inblock && /^crates/ {
    s=$0; sub(/^[^=]*=\s*\[/,"",s); sub(/\]\s*$/,"",s)
    gsub(/[ \t"]/,"",s); n=split(s,arr,",")
    for (i=1;i<=n;i++) print arr[i]
  }
' "$manifest")

fail=0
for c in "${crates[@]}"; do
  if crate_version_present "$c" "$version"; then
    printf '✓ %s@%s\n' "$c" "$version"
  else
    printf '❌ %s@%s MISSING\n' "$c" "$version"
    fail=1
  fi
done
exit "$fail"
