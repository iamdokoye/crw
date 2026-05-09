#!/usr/bin/env bash
# Common helpers for release scripts.
# Always source with `set -euo pipefail` already enabled in the caller.

# Run a command, capturing combined stdout+stderr without dying on failure.
# Required because `out=$(cmd)` under `set -e` aborts on non-zero rc, hiding
# the output. Use this whenever you need to inspect the failure text.
#
# Usage:
#   out=""; rc=0
#   run_capture out rc cargo publish -p crw-core
#   echo "$out"; echo "rc=$rc"
run_capture() {
  local _outvar="$1"; shift
  local _rcvar="$1"; shift
  local _out _rc
  if _out=$("$@" 2>&1); then _rc=0; else _rc=$?; fi
  printf -v "$_outvar" '%s' "$_out"
  printf -v "$_rcvar" '%s' "$_rc"
}

# Test whether `cargo publish` output indicates the version is already on the
# registry. Used for idempotent re-runs.
is_already_uploaded() {
  # shellcheck disable=SC2016 # backticks are literal in cargo's error message
  printf '%s' "$1" | grep -qE 'already (uploaded|exists)|crate version `[^`]+` is already uploaded'
}

# crates.io API: returns 0 if exact <crate>@<version> exists.
crate_version_present() {
  local crate="$1" version="$2"
  curl -fsSL -H "User-Agent: crw-release" \
    "https://crates.io/api/v1/crates/${crate}/${version}" 2>/dev/null \
    | jq -e --arg v "$version" '.version.num == $v' >/dev/null 2>&1
}

# crates.io cksum for an existing version (sha256 of .crate file).
crate_version_cksum() {
  local crate="$1" version="$2"
  curl -fsSL -H "User-Agent: crw-release" \
    "https://crates.io/api/v1/crates/${crate}/${version}" 2>/dev/null \
    | jq -r '.version.cksum // empty'
}

# Parse a workspace member's local version from its Cargo.toml.
# Falls back to root workspace.package.version (members typically inherit).
crate_local_version() {
  local crate="$1" workspace_dir="${2:-.}"
  local manifest="${workspace_dir}/crates/${crate}/Cargo.toml"
  local v
  v=$(grep -E '^version\s*=' "$manifest" | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
  if [ -z "$v" ]; then
    v=$(grep -E '^version\s*=' "${workspace_dir}/Cargo.toml" | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
  fi
  [ -n "$v" ] || { echo "::error::cannot determine version for $crate" >&2; return 1; }
  printf '%s' "$v"
}

# Annotation helpers — visible in GitHub Actions logs.
notice() { printf '::notice::%s\n' "$*"; }
warn()   { printf '::warning::%s\n' "$*"; }
err()    { printf '::error::%s\n' "$*" >&2; }
die()    { err "$*"; exit 1; }
group()    { printf '::group::%s\n' "$*"; }
endgroup() { printf '::endgroup::\n'; }
