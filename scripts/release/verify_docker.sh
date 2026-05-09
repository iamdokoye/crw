#!/usr/bin/env bash
# Verify ghcr.io/us/crw image exists for $version, latest, and major.minor,
# with both linux/amd64 and linux/arm64 manifests.
#
# Usage: verify_docker.sh <version> [image=ghcr.io/us/crw]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

v="${1:?version required}"
image="${2:-ghcr.io/us/crw}"
major_minor=$(printf '%s' "$v" | cut -d. -f1-2)

fail=0
for tag in "$v" "latest" "$major_minor"; do
  manifest=$(docker manifest inspect "${image}:${tag}" 2>/dev/null || echo "")
  if [ -z "$manifest" ]; then
    printf '❌ %s:%s missing\n' "$image" "$tag"
    fail=1
    continue
  fi
  for arch in amd64 arm64; do
    # Compare as `<os>/<arch>` to match docker conventions.
    if printf '%s' "$manifest" \
        | jq -e --arg a "linux/$arch" '
          .manifests[]?
          | (.platform.os + "/" + .platform.architecture) as $p
          | select($p == $a)
          ' >/dev/null 2>&1; then
      printf '✓ %s:%s linux/%s\n' "$image" "$tag" "$arch"
    else
      printf '❌ %s:%s missing linux/%s\n' "$image" "$tag" "$arch"
      fail=1
    fi
  done
done
exit "$fail"
