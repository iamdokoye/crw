"""Minimal stdlib HTTP (no third-party deps) for capture/compare. The SDK
conformance test (`test_sdk.py`) uses the real firecrawl-py client instead."""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.request
from typing import Any

from .corpus import Case

TERMINAL = {"completed", "failed", "cancelled"}


def http_json(
    base: str,
    key: str | None,
    method: str,
    path: str,
    body: dict[str, Any] | None = None,
    timeout: int = 120,
) -> tuple[int, Any]:
    url = base.rstrip("/") + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if key:
        req.add_header("Authorization", f"Bearer {key}")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read().decode())
        except Exception:
            return e.code, {}


def run_case(
    base: str, key: str | None, case: Case, poll_secs: float = 2.0, max_polls: int = 90
) -> tuple[int, Any]:
    """Drive one case: sync returns the POST body; job starts then polls the
    status path until a terminal status (or budget exhausted)."""
    status, body = http_json(base, key, case.method, case.path, case.body)
    if case.kind == "sync" or not isinstance(body, dict):
        return status, body
    job_id = body.get("id")
    if not job_id or not case.status_path_tmpl:
        return status, body
    spath = case.status_path_tmpl.format(id=job_id)
    st, sb = status, body
    for _ in range(max_polls):
        st, sb = http_json(base, key, "GET", spath)
        if isinstance(sb, dict) and str(sb.get("status", "")).lower() in TERMINAL:
            return st, sb
        time.sleep(poll_secs)
    return st, sb
