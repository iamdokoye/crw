#!/usr/bin/env bash
# Verify that an APT/Homebrew dispatch was actually consumed by the
# downstream repo and produced the expected artifact.
#
# Strategy (codex review v4 C5 + v5 W12):
#   1. Search recent commits in each downstream repo for a commit status
#      whose context = "crw-release-<version>" and description = correlation_id.
#   2. Verify the actual artifact landed (deb file in apt-crw release; version
#      bump in homebrew-crw Formula/crw.rb).
#
# Usage: verify_apt_homebrew.sh <version> <correlation_id>
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

v="${1:?version required}"
corr="${2:?correlation_id required}"

end=$((SECONDS + 1800))  # 30min per repo

verify_repo_status() {
  local repo="$1" ctx="crw-release-$v"
  while (( SECONDS < end )); do
    # Search the most recent 20 commits for matching status.
    while read -r sha; do
      [ -z "$sha" ] && continue
      hit=$(gh api "repos/$repo/commits/$sha/statuses" 2>/dev/null \
            | jq -r --arg c "$ctx" --arg d "$corr" \
              '.[]? | select(.context == $c and .description == $d) | .sha' \
            | head -1) || true
      if [ -n "$hit" ]; then
        notice "$repo: matched status $ctx on $sha"
        return 0
      fi
    done < <(gh api "repos/$repo/commits?per_page=20" -q '.[].sha' 2>/dev/null)
    sleep 30
  done
  err "$repo: no status $ctx with correlation_id=$corr found in 30min"
  return 1
}

fail=0
verify_repo_status us/apt-crw      || fail=1
verify_repo_status us/homebrew-crw || fail=1

# Artifact verification (independent of the status check, in case the
# downstream forgot to write the status but still produced the artifact).
if gh release view -R us/apt-crw "v$v" --json assets \
    -q '.assets[].name' 2>/dev/null | grep -q "crw_${v}_amd64.deb"; then
  printf '✓ apt-crw release v%s has crw_%s_amd64.deb\n' "$v" "$v"
else
  printf '❌ apt-crw release v%s missing crw_%s_amd64.deb\n' "$v" "$v"
  fail=1
fi

if gh api repos/us/homebrew-crw/contents/Formula/crw.rb 2>/dev/null \
    | jq -r .content | base64 -d 2>/dev/null | grep -q "version \"$v\""; then
  printf '✓ homebrew-crw Formula/crw.rb pinned to %s\n' "$v"
else
  printf '❌ homebrew-crw Formula/crw.rb does not contain version "%s"\n' "$v"
  fail=1
fi

exit "$fail"
