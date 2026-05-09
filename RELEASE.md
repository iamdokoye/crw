# Release Operations

How crw-opencore ships, what to do when something breaks, and how to keep the pipeline honest.

## Topology

`release-please` watches `main` for conventional commits and opens a Release PR. Merging it creates the `vX.Y.Z` tag, which fires `.github/workflows/release.yml`. That workflow fans out to:

| Target           | Job                  | Verification                                           |
| ---------------- | -------------------- | ------------------------------------------------------ |
| crates.io        | `publish-crates`     | `verify_crates.sh` — exact-version GET on every crate  |
| GitHub Release   | `publish-binaries`   | `asset-gate` (18 tar.gz/zip files attached)            |
| PyPI             | `publish-pypi`       | `verify_pypi.sh`                                       |
| npm              | `publish-npm`        | `verify_npm.sh` — existence + optionalDeps pin + smoke |
| GHCR (Docker)    | `publish-docker`     | `verify_docker.sh` — version + latest + major.minor    |
| MCP Registry     | `publish-mcp-registry` | `verify_mcp_registry.sh`                             |
| APT repo (us/apt-crw) | `update-apt`    | `verify_apt_homebrew.sh` — commit-status correlation   |
| Homebrew tap (us/homebrew-crw) | `update-homebrew` | same                                       |

Each tag also produces `release-audit-<version>.md` as a workflow artifact and attaches it to the GitHub Release.

## Source of truth

- **Tier order & publish flags:** `scripts/release/release_manifest.toml`
- **Workspace publishability:** `scripts/release/preflight.py` (run on every PR via `preflight-publish.yml`)
- **release-please extra-files validity:** `scripts/release/audit_release_please_config.py`
- **Tag → version derivation:** `release-context` job in `release.yml` (semver-validated)

## Recovery runbooks

### A registry failed mid-release

The pipeline is idempotent — re-run is safe.

- **crates.io**: `publish_crate.sh` treats "already uploaded" as success only when the local `.crate` sha256 matches the registry cksum. Mismatch = hard fail. Workaround: bump patch version.
- **PyPI**: `twine upload --skip-existing` ignores duplicates.
- **npm**: idempotent publish — checks `npm view <pkg>@<v>` first.
- **Docker**: GHCR is content-addressed; rebuild + push idempotent.
- **MCP Registry**: re-run `mcp-publisher publish`.
- **APT/Homebrew**: re-trigger via `gh workflow run release.yml --ref main -f tag=vX.Y.Z`.

### A version was published with broken metadata (e.g. wrong npm optionalDeps)

crates.io and npm versions are **immutable**. Do not try to overwrite. Instead:

1. `cargo yank --version X.Y.Z <crate>` (per crate, if you must — yanked versions still work but stop new resolves).
2. `bash scripts/release/deprecate_npm_versions.sh X.Y.Z "<reason>"` to mark all 7 npm packages deprecated.
3. Land fixes on `main`. Conventional commits → release-please opens X.Y.Z+1 PR. Merge it.
4. Verify the new pipeline run leaves green checkmarks across every registry in the audit log.

### v0.4.0–v0.6.0 absence on crates.io

These tags were cut while the release pipeline silently failed (cargo publish output piped to `tee` which always returned 0). They are **not on crates.io and never will be** — crates.io is immutable. v0.6.1 is the first canonical 0.6.x release on crates.io. Earlier versions remain available on GitHub releases / npm / Docker / PyPI under their published artifacts (npm 0.6.0 has the broken optionalDeps pin and is deprecated; install `crw-mcp@^0.6.1`).

## Secret rotation

| Secret               | Used by                          | Rotation                                     |
| -------------------- | -------------------------------- | -------------------------------------------- |
| `CARGO_REGISTRY_TOKEN` | `publish-crates`               | crates.io account → API tokens → revoke + create new with `publish-update` scope |
| `PYPI_TOKEN`         | `publish-pypi`                   | pypi.org account → API tokens → scoped to project `crw` |
| `NPM_TOKEN`          | `publish-npm`                    | npmjs.com → access tokens → automation token |
| `GH_DISPATCH_PAT`    | `dispatch-release`, `update-apt`, `update-homebrew` | GitHub fine-grained PAT with `actions:write` on this repo + `us/apt-crw` + `us/homebrew-crw` |

`GITHUB_TOKEN` is auto-provisioned and does not need rotation.

## Adding a new publishable crate

1. Add the crate to the workspace as usual (no `publish` field, or `publish = true`).
2. Decide its tier: which tier-N crates does it depend on? Add it to the next tier in `scripts/release/release_manifest.toml`.
3. If it has its own version surfaces (`Cargo.toml` deps in other crates), add `extra-files` entries in `release-please-config.json`. Run `uv run python scripts/release/audit_release_please_config.py` to confirm the jsonpaths resolve.
4. **First-publish ownership:** the user behind `CARGO_REGISTRY_TOKEN` becomes the initial owner. Verify the crate name is unclaimed — `curl -s https://crates.io/api/v1/crates/<name>` should return `{"errors":[{"detail":"Not Found"}]}`. After the first publish, optionally `cargo owner --add github:us:release-bots <name>`.
5. Open a PR. `preflight-publish.yml` runs the full workspace check — green means the crate is publishable.

## Adding a new internal (non-published) crate

1. `[package] publish = false` in its Cargo.toml.
2. Add to `scripts/release/release_manifest.toml` under `[unpublished]`.
3. Preflight enforces both must agree.
