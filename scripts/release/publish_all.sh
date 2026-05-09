#!/usr/bin/env bash
# Drives the full crates.io publish in tier order from release_manifest.toml.
# Each tier publishes its crates, then waits for crates.io index propagation
# before starting the next tier.
#
# Usage: publish_all.sh --version <X.Y.Z> [--source-dir DIR] [--manifest PATH]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

version=""; source_dir="."; manifest="$SCRIPT_DIR/release_manifest.toml"
while [ $# -gt 0 ]; do
  case "$1" in
    --version)     version="$2"; shift 2 ;;
    --source-dir)  source_dir="$2"; shift 2 ;;
    --manifest)    manifest="$2"; shift 2 ;;
    *) die "unknown arg: $1" ;;
  esac
done
[ -n "$version" ] || die "--version required"
[ -f "$manifest" ] || die "manifest not found: $manifest"

# Parse tiers from TOML. Avoid pulling in tomlq; do a small awk parser scoped
# to known structure (each [[tiers]] block has `crates = [...]`).
mapfile -t tier_lines < <(awk '
  /^\[\[tiers\]\]/   { inblock=1; crates=""; order=""; next }
  inblock && /^order/ { gsub(/[^0-9]/,""); order=$0; next }
  inblock && /^crates/ {
    s=$0; sub(/^[^=]*=\s*\[/,"",s); sub(/\]\s*$/,"",s)
    gsub(/[ \t"]/,"",s); crates=s
  }
  inblock && /^$/    { if (order!="" && crates!="") print order"\t"crates; inblock=0 }
  END { if (inblock && order!="" && crates!="") print order"\t"crates }
' "$manifest")

[ ${#tier_lines[@]} -gt 0 ] || die "no tiers parsed from $manifest"

# Sort by tier order
mapfile -t sorted < <(printf '%s\n' "${tier_lines[@]}" | sort -k1n)

for line in "${sorted[@]}"; do
  order="${line%%	*}"
  csv="${line#*	}"
  IFS=',' read -ra crates <<<"$csv"
  group "Tier $order: publishing ${crates[*]}"
  for crate in "${crates[@]}"; do
    "$SCRIPT_DIR/publish_crate.sh" "$crate" "$version" --source-dir "$source_dir"
  done
  endgroup
  group "Tier $order: waiting for crates.io index"
  for crate in "${crates[@]}"; do
    "$SCRIPT_DIR/wait_for_crate_version.sh" "$crate" "$version"
  done
  endgroup
done

notice "all tiers published @ $version"
