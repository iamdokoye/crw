#!/usr/bin/env bash
# Verify the standalone npm SDK package (crw-sdk):
#   1. Existence of crw-sdk@version
#   2. Install + dual-format import smoke (CommonJS require + ESM import)
#
# Usage: verify_npm_sdk.sh <version>
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

v="${1:?version required}"
pkg="crw-sdk" # single source: keep in sync with sdks/typescript/package.json "name"

# 1. Existence — poll, since npm registry propagation can lag several seconds
# after a fresh publish (otherwise this false-fails a successful release).
actual="MISSING"
for _ in 1 2 3 4 5 6; do
  actual=$(npm view "$pkg@$v" version 2>/dev/null || echo "MISSING")
  [ "$actual" = "$v" ] && break
  sleep 10
done
[ "$actual" = "$v" ] || die "$pkg@$v not on npm after retries (got: $actual)"

# 2. Install + dual-format import smoke
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
(cd "$tmp" && npm init -y >/dev/null && npm install --silent "$pkg@$v" >/dev/null 2>&1) \
  || die "npm install $pkg@$v failed"
(cd "$tmp" && node -e "const {CrwClient}=require('$pkg'); if(typeof CrwClient!=='function') process.exit(1)") \
  || die "CJS require smoke failed for $pkg@$v"
(cd "$tmp" && node --input-type=module -e "import {CrwClient} from '$pkg'; if(typeof CrwClient!=='function') process.exit(1)") \
  || die "ESM import smoke failed for $pkg@$v"

notice "verify_npm_sdk OK: $pkg@$v"
