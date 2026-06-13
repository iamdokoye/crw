# Proxy Rotation — Multi-Agent Review Fix Plan

Review: 17 findings raised, 17 confirmed (0 refuted). Deduplicated below. Many
collapse into ONE unifying fix: **resolve the per-request proxy exactly once
(BYOP > config), store it in `REQUEST_PROXY`, and make BOTH the HTTP and CDP
paths consume that single entry** — no re-picking, no path-specific resolution.

## Unifying refactor (fixes A, D, E, parts of integration)

1. **`crw-core/proxy.rs`** — make `StickyPerHost` **stateless**: `idx = fxhash(host) % len`.
   Removes the `Mutex<HashMap>` (→ fixes #13 unbounded growth) and the first-insert
   `next_rr()` advance (→ removes a cursor side-effect). Replace `debug_assert!(len>0)`
   with a real guard / keep build the sole non-empty constructor (#14).
2. **Single resolution point.** Add `ProxyRotator::pick` usage so callers resolve ONE
   `ProxyEntry`:
   - `single.rs::scrape_url`: build the BYOP rotator from `req.proxy_list`/`req.proxy`
     FIRST (InvalidRequest on bad); `resolved = byop.pick(host) ?? renderer.pick_proxy_for_url(url)`;
     scope `REQUEST_PROXY(resolved)`. (Fixes A/#1/#4/#9.)
   - `crawl.rs`: per page build BYOP rotator from `CrawlRequest.proxy_list` (job-scoped,
     fail the job on bad) and resolve `REQUEST_PROXY` from it, else config. (Fixes E/#8.)
3. **HTTP path honors `REQUEST_PROXY`.** `RotatingHttpFetcher::fetch`: if `REQUEST_PROXY`
   is set, use the warm client whose `entry.raw()` matches; if absent (BYOP not in the
   config pool) build a one-off client; only when `REQUEST_PROXY` is `None` fall back to
   `rotator.pick_index`. (Fixes D/#11/#12 — single pick, HTTP+CDP aligned.) Also make the
   plain (no-config) HTTP path honor `REQUEST_PROXY` so BYOP-with-no-config-proxy is
   proxied, never direct.
4. Drop the now-redundant per-request temp-fetcher branch in `single.rs` (the shared
   renderer + `REQUEST_PROXY` covers proxy; keep the temp fetcher only for the
   stealth-only override case).

## Fail-closed fixes (blockers/highs)

5. **B/#2/#3 — LightPanda escape hatch.** In `fetch_with_js`, when `proxy_active` and
   filtering removes all renderers, return a hard `CrwError` ("proxy required but no
   proxy-capable JS renderer"), never keep LightPanda. Add `PageFetcher::supports_proxy()`
   (default true; LightPanda false) to generalize.
6. **C/#5/#6 — CLI silent fallback.** Route CLI `--proxy` through `ProxyRotator::build` +
   `with_proxy_rotator` (like the server), and/or split `HttpFetcher`: infallible no-proxy
   ctor + fallible proxy ctor. Remove the `Option<&str> proxy → unwrap_or_else(default
   client)` foot-gun so a bad proxy is a hard error everywhere.
7. **F/#10 — robots/discovery.** Build the robots/discovery reqwest client from the
   rotator (pick for the origin host) instead of the single `proxy`; replace the
   `warn + continue direct` on bad proxy with a hard error.
8. **G/#7 — SOCKS5+auth on CDP.** In `ProxyEntry`, expose `supports_cdp_auth()` (false for
   socks5/socks5h with auth) and map `socks5h`→`socks5` for `chrome_proxy_server`. Reject
   (or skip-to-error) a socks5+auth entry on the CDP path so it never silently hangs.

## Low / hardening
- #15 dispose the browser context on the malformed-`createBrowserContext` parse-failure path.
- #16 `proxyBypassList`: omit it (Chrome bypasses loopback by default) or use `"<local>"`.
- #17 map `RotatingHttpFetcher` build error to `InvalidRequest` on the BYOP call site.

## Verify
- `cargo clippy --workspace` (default + `cdp`) clean; full test suite green.
- New unit tests: stateless-sticky determinism; round_robin single-advance per request;
  socks5+auth → rejected for CDP; socks5h→socks5 mapping; lightpanda-only+proxy → error;
  proxy-active prefers chrome over lightpanda; `HttpFetcher::with_proxy` fail-closed.
- Re-run the multi-agent review workflow until 0 confirmed blocker/high findings.

## Review loop outcome (CONVERGED)

- **Round 1** — 17 confirmed (3 blocker, then highs/mediums/lows). Fixed in commits
  `92c5027` + `3ff9145`.
- **Round 2** — 11 confirmed, **4 high** — all one root: the `/map` `discover_urls`
  path was left on the old direct/single-proxy path (BFS fetch unscoped + discovery
  client ignored the rotator + silent-swallow). Fixed in `c920a32`.
- **Round 3** — 8 confirmed, **0 blocker/high → `satisfied: true`**. Per-request
  client warm-pool regression and the missing fail-closed tests were then addressed.

### Remaining documented follow-ups (low / nit — non-blocking)
- **#17 error class:** `build_client` maps a post-`parse` reqwest build failure to
  `ConfigError` (HTTP 500) even on the per-request BYOP path (should be
  `InvalidRequest`/400). Window is tiny — `ProxyEntry::parse` already validated
  scheme+host, so `reqwest::Proxy::all` failing afterward is unlikely. Map at the
  `with_proxy` call sites if surfaced.
- **chrome_proxy tier override:** when a config/BYOP `REQUEST_PROXY` is active AND a
  request escalates to the residential `chrome_proxy` tier, the per-request proxy
  overrides the DataImpulse residential egress. Uncommon combo; document or skip the
  per-request context when `self.name == "chrome_proxy"`.
- **sticky key normalization:** BYOP sticky uses the raw host; the config rotator uses
  `normalize_host` (eTLD+1). Mutually exclusive per request (no correctness impact);
  unify by normalizing the BYOP host key too.
