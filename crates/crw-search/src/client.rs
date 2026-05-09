//! HTTP client for SearXNG's JSON search API.
//!
//! Mirrors `crw-saas/src/lib/searxng-client.ts`. The shape of the response
//! follows the SearXNG `search_api` docs and the `result_types/index` page —
//! every per-result field except `url`, `title`, and `engine` is treated as
//! optional because real-world engines are uneven.

use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

use crate::params::SearxngParams;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("SearXNG request timed out")]
    Timeout,
    #[error("SearXNG upstream error (status {status}): {body}")]
    Upstream { status: u16, body: String },
    #[error("SearXNG returned an invalid JSON response: {0}")]
    InvalidResponse(String),
    #[error("SearXNG transport error: {0}")]
    Transport(String),
}

/// A single result row from SearXNG. All fields except `url`, `title`, and
/// `engine` are nullable — engines vary widely in what they populate.
#[derive(Debug, Clone, Deserialize)]
pub struct SearxngResult {
    pub url: String,
    pub title: String,
    pub engine: String,
    /// Snippet / description. SearXNG calls this `content`; the public API
    /// renames it to `description`.
    #[serde(default)]
    pub content: Option<String>,
    /// Relevance score (higher is better). Missing on engines that don't rank.
    #[serde(default)]
    pub score: Option<f64>,
    /// Top-level category bucket reported by SearXNG (`general`, `news`,
    /// `images`, `videos`, ...).
    #[serde(default)]
    pub category: Option<String>,
    /// Template hint (`default.html`, `images.html`, `videos.html`,
    /// `paper.html`, ...). Useful as a fallback when `category` is missing.
    #[serde(default)]
    pub template: Option<String>,
    /// ISO-formatted publish date for news results.
    #[serde(default, rename = "publishedDate")]
    pub published_date: Option<String>,
    /// Image URL — populated for image-template results.
    #[serde(default)]
    pub img_src: Option<String>,
    /// Thumbnail URL — populated for image / video results.
    #[serde(default)]
    pub thumbnail_src: Option<String>,
    #[serde(default)]
    pub img_format: Option<String>,
    #[serde(default)]
    pub resolution: Option<String>,
}

/// Top-level SearXNG `format=json` response envelope.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SearxngResponse {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub number_of_results: u64,
    #[serde(default)]
    pub results: Vec<SearxngResult>,
    #[serde(default)]
    pub answers: Vec<serde_json::Value>,
    #[serde(default)]
    pub corrections: Vec<String>,
    #[serde(default)]
    pub infoboxes: Vec<serde_json::Value>,
    #[serde(default)]
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub unresponsive_engines: Vec<serde_json::Value>,
}

/// Thin async client for SearXNG. One instance per server; reuse across
/// requests so the underlying `reqwest::Client` connection pool is hot.
#[derive(Debug, Clone)]
pub struct SearxngClient {
    http: Arc<reqwest::Client>,
    base_url: String,
    timeout: Duration,
}

impl SearxngClient {
    pub fn new(http: Arc<reqwest::Client>, base_url: impl Into<String>, timeout: Duration) -> Self {
        let base_url = base_url.into();
        let trimmed = base_url.trim_end_matches('/').to_string();
        Self {
            http,
            base_url: trimmed,
            timeout,
        }
    }

    /// Issue a JSON search request. Errors surface as a typed [`SearchError`]
    /// — the route layer maps them onto `CrwError` for HTTP responses.
    pub async fn fetch(&self, params: &SearxngParams) -> Result<SearxngResponse, SearchError> {
        let mut url = url::Url::parse(&format!("{}/search", self.base_url))
            .map_err(|e| SearchError::Transport(format!("invalid base_url: {e}")))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("format", "json");
            q.append_pair("q", &params.q);
            if let Some(c) = &params.categories {
                q.append_pair("categories", c);
            }
            if let Some(l) = &params.language {
                q.append_pair("language", l);
            }
            if let Some(t) = &params.time_range {
                q.append_pair("time_range", t);
            }
            if let Some(e) = &params.engines {
                q.append_pair("engines", e);
            }
            if let Some(p) = params.pageno {
                q.append_pair("pageno", &p.to_string());
            }
            if let Some(s) = params.safesearch {
                q.append_pair("safesearch", &s.to_string());
            }
        }

        let response = self
            .http
            .get(url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e: reqwest::Error| {
                if e.is_timeout() {
                    SearchError::Timeout
                } else {
                    SearchError::Transport(e.to_string())
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let trimmed: String = body.chars().take(500).collect();
            return Err(SearchError::Upstream {
                status: status.as_u16(),
                body: trimmed,
            });
        }

        let text = response
            .text()
            .await
            .map_err(|e: reqwest::Error| SearchError::Transport(e.to_string()))?;
        serde_json::from_str::<SearxngResponse>(&text)
            .map_err(|e| SearchError::InvalidResponse(e.to_string()))
    }
}
