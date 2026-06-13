//! Proxy list + rotation primitives shared across the HTTP, crawl, and CDP
//! paths.
//!
//! A [`ProxyRotator`] holds a set of validated [`ProxyEntry`] and selects one
//! per request according to a [`ProxyRotation`] strategy. The rotator is built
//! once (from config) or per request (BYOP) and is cheap to share behind an
//! `Arc`.
//!
//! # Safety
//!
//! Proxy URLs are validated up front via [`ProxyEntry::parse`]. A malformed
//! entry is a hard error — we never silently fall back to a direct (no-proxy)
//! connection, which would leak the host's real IP. Callers map the returned
//! error string to the appropriate [`crate::CrwError`] variant
//! (`ConfigError` at startup, `InvalidRequest` for per-request BYOP).

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

/// Strategy for selecting a proxy from a [`ProxyRotator`]'s pool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRotation {
    /// Cycle through the pool in order, one step per request (process-wide).
    RoundRobin,
    /// Pick a uniformly random entry per request.
    Random,
    /// Pin each target host to a single proxy for the rotator's lifetime.
    /// Default: keeps cookie/TLS sessions coherent per host (anti-bot systems
    /// flag mid-session IP changes), while still spreading load across hosts.
    #[default]
    StickyPerHost,
}

/// A single validated proxy endpoint.
///
/// `raw` carries the full URL (including any `user:pass`) for `reqwest`, which
/// honours embedded credentials. `chrome_proxy_server` is the scheme-qualified
/// `host:port` **without** credentials, suitable for Chrome's
/// `Target.createBrowserContext { proxyServer }` (Chrome takes creds via the
/// `Fetch.authRequired` auth pump, not in the URL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEntry {
    raw: String,
    scheme: String,
    chrome_proxy_server: String,
    auth: Option<(String, String)>,
}

const ALLOWED_SCHEMES: [&str; 4] = ["http", "https", "socks5", "socks5h"];

impl ProxyEntry {
    /// Parse and validate a proxy URL. Returns an error string (no silent
    /// fallback) when the scheme is unsupported or the host is missing.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("empty proxy URL".to_string());
        }
        let url =
            url::Url::parse(trimmed).map_err(|e| format!("invalid proxy URL '{trimmed}': {e}"))?;

        let scheme = url.scheme().to_ascii_lowercase();
        if !ALLOWED_SCHEMES.contains(&scheme.as_str()) {
            return Err(format!(
                "unsupported proxy scheme '{scheme}' in '{trimmed}' (allowed: http, https, socks5, socks5h)"
            ));
        }

        let host = url
            .host_str()
            .ok_or_else(|| format!("proxy URL '{trimmed}' has no host"))?;

        let chrome_proxy_server = match url.port() {
            Some(port) => format!("{scheme}://{host}:{port}"),
            None => format!("{scheme}://{host}"),
        };

        let auth = match (url.username(), url.password()) {
            ("", _) => None,
            (user, Some(pass)) => Some((percent_decode(user), percent_decode(pass))),
            (user, None) => Some((percent_decode(user), String::new())),
        };

        Ok(Self {
            raw: trimmed.to_string(),
            scheme,
            chrome_proxy_server,
            auth,
        })
    }

    /// Full proxy URL (with credentials) for `reqwest::Proxy::all`.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// URL scheme (lowercased): `http`, `https`, `socks5`, or `socks5h`.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Scheme-qualified `host:port` (no credentials) for Chrome `proxyServer`.
    pub fn chrome_proxy_server(&self) -> &str {
        &self.chrome_proxy_server
    }

    /// Optional `(username, password)` for the CDP auth pump.
    pub fn auth(&self) -> Option<&(String, String)> {
        self.auth.as_ref()
    }
}

/// Minimal percent-decoding for proxy userinfo (handles `%XX`). Credentials
/// frequently contain URL-encoded characters; `url` exposes them encoded.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// A pool of validated proxies plus a selection strategy.
///
/// Construct via [`ProxyRotator::build`]. Returns `Ok(None)` when there are no
/// proxies (caller then connects directly, preserving today's behaviour).
#[derive(Debug)]
pub struct ProxyRotator {
    entries: Vec<ProxyEntry>,
    strategy: ProxyRotation,
    rr_cursor: AtomicUsize,
    sticky: Mutex<HashMap<String, usize>>,
}

impl ProxyRotator {
    /// Build a rotator with precedence: a non-empty `list` wins; otherwise the
    /// single `single` proxy becomes a pool of one; otherwise `Ok(None)`.
    ///
    /// Every entry is validated — any malformed URL is a hard error (no silent
    /// no-proxy fallback). The error is a human-readable string the caller maps
    /// to a [`crate::CrwError`].
    pub fn build(
        list: &[String],
        single: Option<&str>,
        strategy: ProxyRotation,
    ) -> Result<Option<Self>, String> {
        let raws: Vec<&str> = if !list.is_empty() {
            list.iter().map(String::as_str).collect()
        } else if let Some(s) = single.map(str::trim).filter(|s| !s.is_empty()) {
            vec![s]
        } else {
            return Ok(None);
        };

        let mut entries = Vec::with_capacity(raws.len());
        for raw in raws {
            entries.push(ProxyEntry::parse(raw)?);
        }
        if entries.is_empty() {
            return Ok(None);
        }

        Ok(Some(Self {
            entries,
            strategy,
            rr_cursor: AtomicUsize::new(0),
            sticky: Mutex::new(HashMap::new()),
        }))
    }

    /// Number of proxies in the pool.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Always false — `build` returns `None` for empty pools.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The validated entries, in stable index order. Callers that pre-build a
    /// per-entry resource (e.g. one `reqwest::Client` per proxy) index into
    /// this and select with [`Self::pick_index`].
    pub fn entries(&self) -> &[ProxyEntry] {
        &self.entries
    }

    /// Select a proxy for a request. `host_key` is used only by
    /// [`ProxyRotation::StickyPerHost`]; pass the normalized target host.
    pub fn pick(&self, host_key: Option<&str>) -> &ProxyEntry {
        &self.entries[self.pick_index(host_key)]
    }

    /// Index into [`Self::entries`] for this request, applying the strategy.
    pub fn pick_index(&self, host_key: Option<&str>) -> usize {
        let len = self.entries.len();
        debug_assert!(len > 0, "ProxyRotator must never be empty");
        match self.strategy {
            ProxyRotation::RoundRobin => self.next_rr() % len,
            ProxyRotation::Random => rand::random_range(0..len),
            ProxyRotation::StickyPerHost => match host_key {
                Some(host) => {
                    let mut map = self.sticky.lock().unwrap_or_else(|e| e.into_inner());
                    *map.entry(host.to_string())
                        .or_insert_with(|| self.next_rr() % len)
                }
                None => self.next_rr() % len,
            },
        }
    }

    fn next_rr(&self) -> usize {
        self.rr_cursor.fetch_add(1, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_with_auth() {
        let e = ProxyEntry::parse("http://user:pass@host.example:8080").unwrap();
        assert_eq!(e.scheme(), "http");
        assert_eq!(e.chrome_proxy_server(), "http://host.example:8080");
        assert_eq!(e.auth(), Some(&("user".to_string(), "pass".to_string())));
        assert_eq!(e.raw(), "http://user:pass@host.example:8080");
    }

    #[test]
    fn parse_socks5_no_auth() {
        let e = ProxyEntry::parse("socks5://1.2.3.4:1080").unwrap();
        assert_eq!(e.scheme(), "socks5");
        assert_eq!(e.chrome_proxy_server(), "socks5://1.2.3.4:1080");
        assert!(e.auth().is_none());
    }

    #[test]
    fn parse_percent_encoded_auth() {
        let e = ProxyEntry::parse("http://u%40b:p%3Aw@h:8080").unwrap();
        assert_eq!(e.auth(), Some(&("u@b".to_string(), "p:w".to_string())));
    }

    #[test]
    fn parse_rejects_bad_scheme() {
        assert!(ProxyEntry::parse("ftp://h:21").is_err());
        assert!(ProxyEntry::parse("not a url").is_err());
        assert!(ProxyEntry::parse("").is_err());
    }

    #[test]
    fn build_empty_is_none() {
        assert!(
            ProxyRotator::build(&[], None, ProxyRotation::RoundRobin)
                .unwrap()
                .is_none()
        );
        assert!(
            ProxyRotator::build(&[], Some("  "), ProxyRotation::RoundRobin)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn build_single_is_pool_of_one() {
        let r = ProxyRotator::build(&[], Some("http://h:8080"), ProxyRotation::RoundRobin)
            .unwrap()
            .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r.pick(None).chrome_proxy_server(), "http://h:8080");
    }

    #[test]
    fn build_list_wins_over_single() {
        let list = vec!["http://a:1".to_string(), "http://b:2".to_string()];
        let r = ProxyRotator::build(&list, Some("http://single:9"), ProxyRotation::RoundRobin)
            .unwrap()
            .unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn build_bad_entry_is_hard_error() {
        let list = vec!["http://ok:1".to_string(), "ftp://bad:2".to_string()];
        assert!(ProxyRotator::build(&list, None, ProxyRotation::RoundRobin).is_err());
    }

    #[test]
    fn round_robin_cycles_in_order() {
        let list = vec![
            "http://a:1".to_string(),
            "http://b:2".to_string(),
            "http://c:3".to_string(),
        ];
        let r = ProxyRotator::build(&list, None, ProxyRotation::RoundRobin)
            .unwrap()
            .unwrap();
        let seq: Vec<&str> = (0..4).map(|_| r.pick(None).raw()).collect();
        assert_eq!(
            seq,
            vec!["http://a:1", "http://b:2", "http://c:3", "http://a:1"]
        );
    }

    #[test]
    fn random_stays_in_bounds() {
        let list = vec!["http://a:1".to_string(), "http://b:2".to_string()];
        let r = ProxyRotator::build(&list, None, ProxyRotation::Random)
            .unwrap()
            .unwrap();
        for _ in 0..100 {
            let raw = r.pick(None).raw();
            assert!(raw == "http://a:1" || raw == "http://b:2");
        }
    }

    #[test]
    fn sticky_pins_host_to_one_proxy() {
        let list = vec![
            "http://a:1".to_string(),
            "http://b:2".to_string(),
            "http://c:3".to_string(),
        ];
        let r = ProxyRotator::build(&list, None, ProxyRotation::StickyPerHost)
            .unwrap()
            .unwrap();
        let first = r.pick(Some("example.com")).raw().to_string();
        for _ in 0..50 {
            assert_eq!(r.pick(Some("example.com")).raw(), first);
        }
        // A different host may land on a different proxy, but is itself stable.
        let other = r.pick(Some("other.com")).raw().to_string();
        for _ in 0..50 {
            assert_eq!(r.pick(Some("other.com")).raw(), other);
        }
    }

    #[test]
    fn default_strategy_is_sticky() {
        assert_eq!(ProxyRotation::default(), ProxyRotation::StickyPerHost);
    }
}
