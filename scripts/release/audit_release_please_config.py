#!/usr/bin/env python3
"""Validate every extra-files entry in release-please-config.json.

Catches the class of regression that hid the npm optionalDependencies pin
problem: stale jsonpaths that point at fields that no longer exist (e.g.
`crates/crw-core/Cargo.toml::$.dependencies.crw-core.version`) silently
no-op during release-please runs and leave version surfaces stale.

Supports json + toml + bare string forms (codex review v6 C13).
"""
from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path


def _toml_jsonpath_lookup(data: dict, jsonpath: str) -> bool:
    """Naive `$.a.b.c` walk over a TOML-loaded dict.

    release-please's TOML extra-files use simple dotted paths; we don't need
    the full jsonpath-ng grammar. Anything more exotic would need to be
    handled explicitly.
    """
    parts = re.findall(r"[\w-]+", jsonpath)
    cur = data
    for seg in parts:
        if isinstance(cur, dict) and seg in cur:
            cur = cur[seg]
        else:
            return False
    return True


def _json_jsonpath_lookup(data, jsonpath: str) -> bool:
    try:
        import jsonpath_ng  # type: ignore
    except ImportError:
        # Fallback: same naive walker. Good enough for `$.a['b'].c` shapes.
        # Strip $ and quotes/brackets, treat as dotted.
        flat = jsonpath.replace("$.", "").replace("$", "")
        flat = re.sub(r"\[['\"]?([^'\"\]]+)['\"]?\]", r".\1", flat)
        parts = [p for p in flat.split(".") if p]
        cur = data
        for seg in parts:
            if isinstance(cur, dict) and seg in cur:
                cur = cur[seg]
            else:
                return False
        return True
    return bool(jsonpath_ng.parse(jsonpath).find(data))


def main(config_path: Path = Path("release-please-config.json")) -> int:
    if not config_path.exists():
        print(f"::error::{config_path} not found", file=sys.stderr)
        return 1
    cfg = json.loads(config_path.read_text())

    errors: list[str] = []
    for pkg_name, pkg in cfg.get("packages", {}).items():
        for ef in pkg.get("extra-files", []):
            # Bare string form: just a path, no jsonpath.
            if isinstance(ef, str):
                if not Path(ef).exists():
                    errors.append(f"{ef}: file missing (string-form extra-file)")
                continue

            path_str = ef.get("path")
            if not path_str:
                errors.append(f"{pkg_name}: extra-file missing 'path'")
                continue
            p = Path(path_str)
            if not p.exists():
                errors.append(f"{path_str}: file missing")
                continue

            t = ef.get("type", "generic")
            jsonpath = ef.get("jsonpath")

            if t == "json":
                if not jsonpath:
                    errors.append(f"{path_str}: type=json missing jsonpath")
                    continue
                try:
                    data = json.loads(p.read_text())
                except json.JSONDecodeError as e:
                    errors.append(f"{path_str}: invalid JSON: {e}")
                    continue
                if not _json_jsonpath_lookup(data, jsonpath):
                    errors.append(f"{path_str}::{jsonpath} (json): jsonpath not found")
            elif t == "toml":
                if not jsonpath:
                    errors.append(f"{path_str}: type=toml missing jsonpath")
                    continue
                try:
                    data = tomllib.loads(p.read_text())
                except tomllib.TOMLDecodeError as e:
                    errors.append(f"{path_str}: invalid TOML: {e}")
                    continue
                if not _toml_jsonpath_lookup(data, jsonpath):
                    errors.append(f"{path_str}::{jsonpath} (toml): jsonpath not found")
            elif t in ("generic", "yaml", "xml"):
                # generic uses regex against file contents; cannot statically
                # validate without re-implementing release-please. Skip.
                continue
            else:
                errors.append(f"{path_str}: unknown extra-file type '{t}'")

    if errors:
        print("::error::release-please-config.json audit failed:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    print(f"::notice::release-please-config.json audit OK ({sum(len(p.get('extra-files', [])) for p in cfg.get('packages', {}).values())} entries)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
