#!/usr/bin/env bash
# Mechanical guard against the stale-internal-dependency-version release break.
#
# Every internal crate is published to crates.io, so its sibling path
# dependencies carry a `version = "X"` compatibility assertion alongside
# `path = "..."`. That `X` MUST equal the unified workspace version
# (`[workspace.package].version`). If a release bumps the workspace version
# but leaves a sibling `version` string behind, cargo can no longer resolve
# the path crate (`^old` excludes the new version) and every publish job in
# the Release workflow is skipped — shipping an empty tag.
#
# This is exactly what happened to v0.9.0: crw-cli's crw-search/crw-server/
# crw-browse deps stayed at "0.8.1" while the workspace went to 0.9.0. The
# heavy `cargo build` CI step did not block the release-please PR (Cargo.lock
# cache short-circuited resolution), so this dedicated, cache-proof invariant
# check runs on every PR — including the release-please version-bump PR,
# where a mismatch first becomes visible — turning a catastrophic post-tag
# failure into a pre-merge red X.
#
# The internal-crate set is derived from `[workspace] members`, so this
# guard never needs editing when crates are added or removed.
#
# Portable: bash + python3 (present on ubuntu-latest and macOS).
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
import re, sys, glob, os

root = open("Cargo.toml").read()

m = re.search(r'\[workspace\.package\][^\[]*?\bversion\s*=\s*"([^"]+)"', root, re.S)
if not m:
    print("error: could not find [workspace.package] version in root Cargo.toml")
    sys.exit(2)
ws_version = m.group(1)

# Internal crate names = basenames of [workspace] members.
mm = re.search(r'\[workspace\][^\[]*?members\s*=\s*\[(.*?)\]', root, re.S)
if not mm:
    print("error: could not find [workspace] members in root Cargo.toml")
    sys.exit(2)
members = re.findall(r'"([^"]+)"', mm.group(1))
internal = {os.path.basename(p) for p in members}

problems = []
for f in sorted(glob.glob("crates/*/Cargo.toml")):
    txt = open(f).read()
    # Walk only dependency tables; ignore [package], [features], etc.
    for tm in re.finditer(
        r'^\[(?:target\.[^\]]+\.)?(dependencies|dev-dependencies|build-dependencies)\]\s*$',
        txt, re.M):
        start = tm.end()
        nxt = re.search(r'^\[', txt[start:], re.M)
        block = txt[start:start + (nxt.start() if nxt else len(txt))]
        for dm in re.finditer(r'^([A-Za-z0-9_-]+)\s*=\s*\{([^}]*)\}', block, re.M):
            name, body = dm.group(1), dm.group(2)
            if name not in internal:
                continue
            if "path" not in body:
                continue
            vm = re.search(r'version\s*=\s*"([^"]+)"', body)
            if not vm:
                continue  # path-only dep: no version assertion to keep in sync
            ver = vm.group(1)
            if ver != ws_version:
                problems.append((f, name, ver))

if problems:
    print(f"❌ internal dependency version drift (workspace version is {ws_version}):\n")
    for f, name, ver in problems:
        print(f'  {f}: {name} = {{ ..., version = "{ver}" }}  →  must be "{ws_version}"')
    print(
        "\nFix: set each listed `version` to the workspace version, and add the "
        "matching entry to release-please-config.json `extra-files` so future "
        "releases keep it in sync automatically."
    )
    sys.exit(1)

print(f"✅ all internal path-dependency versions match workspace version {ws_version}")
PY
