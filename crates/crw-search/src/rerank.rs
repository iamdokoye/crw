//! Content-aware re-ranking pipeline for the LLM "answer" / "summarize"
//! search path.
//!
//! SearXNG's raw `.score` is rank-inverse and content-blind: a `bing`
//! keyword match on a stopword ("top" / "best" / "fix") lets dictionary,
//! shopping, and bot-check pages tie or outrank the real results. Sorting by
//! that score feeds junk to the LLM. This module replaces the raw sort with a
//! CPU-only pipeline that fuses per-engine ranks, drops junk, gates on query
//! coverage, applies a geo signal, and dedupes by registrable domain.
//!
//! Direct port of the proven Python reference in
//! `tests/fixtures/bench/{rerank,score}.py` (`rank_full`). The only deliberate
//! deviation: the graceful-degrade fallback keeps the junk filter applied
//! (it only relaxes the coverage / geo guards) so junk can never re-enter the
//! top-N — the reference's `cands = list(rows)` fallback could otherwise leak
//! a dictionary page back in.
//!
//! No network, no new heavy dependencies — `std` + the `url` crate already in
//! the workspace.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::client::SearxngResult;

// ---- tunable knobs (mirror rerank.py) ----
const K_RRF: f64 = 60.0;
const K1: f64 = 1.2;
const B: f64 = 0.5;
const W_RRF: f64 = 1.0;
const W_REL: f64 = 1.0;
const W_GEO: f64 = 0.6;
const MIN_COVERAGE: f64 = 0.5;

/// Query stopwords. Leading filler ("top"/"best") plus connective tokens that
/// would dilute coverage / BM25 if treated as content terms. Mirrors
/// `score.py::STOPWORDS`.
pub static STOPWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "top", "best", "good", "greatest", "finest", "cheapest", "cheap", "the", "a", "an", "in",
        "of", "to", "for", "and", "or", "near", "how", "is", "are", "do", "does", "from", "with",
        "you", "your", "should", "per", "what", "2026", "2025",
    ]
    .into_iter()
    .collect()
});

/// Host-exact junk signatures (dictionary / shopping / news-aggregator /
/// asset hosts). Mirrors `score.py::JUNK_HOSTS`.
static JUNK_HOSTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "merriam-webster.com",
        "dictionary.cambridge.org",
        "usdictionary.com",
        "dictionary.com",
        "vocabulary.com",
        "thefreedictionary.com",
        "collinsdictionary.com",
        "wiktionary.org",
        "zara.com",
        "bestbuy.com",
        "ebay.com",
        "aliexpress.com",
        "foxnews.com",
        "apnews.com",
        "news.google.com",
        "culturedcode.com",
        "thingiverse.com",
        "apps.apple.com",
        "fix.com",
    ]
    .into_iter()
    .collect()
});

const JUNK_HOST_SUFFIXES: &[&str] = &["myshopify.com"];

/// A geo entry: tokens that confirm the intended region, and competing tokens
/// that mark a homonymous wrong region (e.g. "belgrad" forest near Istanbul).
struct GeoEntry {
    region: &'static [&'static str],
    competing: &'static [&'static str],
}

/// Ambiguous toponyms from the corpus. Mirrors `score.py::GEO`. The map key is
/// a token that, when present in the query, selects the entry.
static GEO: LazyLock<HashMap<&'static str, GeoEntry>> = LazyLock::new(|| {
    HashMap::from([
        (
            "belgrad",
            GeoEntry {
                region: &["belgrade", "beograd", "serbia"],
                competing: &["istanbul", "forest", "turkey", "maine", "lakes", "montana"],
            },
        ),
        (
            "lisbon",
            GeoEntry {
                region: &["lisbon", "lisboa", "portugal"],
                competing: &[],
            },
        ),
        (
            "kyoto",
            GeoEntry {
                region: &["kyoto", "japan"],
                competing: &[],
            },
        ),
        (
            "tbilisi",
            GeoEntry {
                region: &["tbilisi", "georgia"],
                competing: &["atlanta"],
            },
        ),
        (
            "danang",
            GeoEntry {
                region: &["nang", "danang", "vietnam"],
                competing: &[],
            },
        ),
        (
            "porto",
            GeoEntry {
                region: &["porto", "portugal"],
                competing: &[],
            },
        ),
        (
            "tokyo",
            GeoEntry {
                region: &["tokyo", "japan"],
                competing: &[],
            },
        ),
        (
            "oaxaca",
            GeoEntry {
                region: &["oaxaca", "mexico"],
                competing: &[],
            },
        ),
        (
            "zurich",
            GeoEntry {
                region: &["zurich", "switzerland", "swiss"],
                competing: &[],
            },
        ),
        (
            "vienna",
            GeoEntry {
                region: &["vienna", "austria", "wien"],
                competing: &["virginia"],
            },
        ),
    ])
});

/// Lowercase + strip combining diacritics (NFKD fold). Mirrors `score.py::norm`.
fn norm(s: &str) -> String {
    // We avoid pulling `unicode-normalization`; the corpus toponyms only need
    // ASCII-folding of the common Latin diacritics that appear in snippets.
    s.to_lowercase()
        .chars()
        .map(fold_diacritic)
        .collect::<String>()
}

/// Best-effort fold of a single combining-Latin character to its base letter.
/// Covers the accents present in the corpus (Beograd, São, Zürich, ...).
fn fold_diacritic(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

/// Tokenize on non-alphanumeric boundaries over the normalized string.
/// Mirrors `score.py::toks`.
fn toks(s: &str) -> Vec<String> {
    norm(s)
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

/// Host of a URL, with a leading `www.` stripped. Mirrors `score.py::domain`.
fn domain(url: &str) -> String {
    // url.split("/")[2] in Python — the authority component.
    let host = url
        .split("//")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

/// Last two labels of the host (registrable-ish). Mirrors
/// `score.py::registrable` — deliberately the same naive two-label rule so the
/// Rust dedupe matches the proven reference exactly. A full PSL would change
/// dedupe behavior on `co.uk`-style suffixes; none appear in the corpus and
/// the reference is the contract we're porting.
fn registrable(url: &str) -> String {
    let d = domain(url);
    let parts: Vec<&str> = d.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        d
    }
}

fn url_of(r: &SearxngResult) -> &str {
    r.url.as_deref().unwrap_or("")
}

fn title_of(r: &SearxngResult) -> &str {
    r.title.as_deref().unwrap_or("")
}

fn content_of(r: &SearxngResult) -> &str {
    r.content.as_deref().unwrap_or("")
}

/// Reciprocal Rank Fusion contribution for one row. Mirrors `rerank.py::rrf`.
fn rrf(r: &SearxngResult) -> f64 {
    if r.positions.is_empty() {
        1.0 / (K_RRF + 1.0) // single unknown-rank vote
    } else {
        r.positions.iter().map(|&p| 1.0 / (K_RRF + p as f64)).sum()
    }
}

/// Build a min-max normalizer closure. Returns a constant 0.0 when the range
/// collapses, matching `rerank.py::minmax`.
fn minmax(vals: &[f64]) -> impl Fn(f64) -> f64 {
    let lo = vals.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let rng = hi - lo;
    move |v: f64| if rng > 1e-9 { (v - lo) / rng } else { 0.0 }
}

/// Title-weighted (2x) token multiset for a row. Mirrors the doc construction
/// in `rerank.py::bm25_lite`.
fn doc_tokens(r: &SearxngResult) -> Vec<String> {
    let mut d = toks(title_of(r));
    d.extend(toks(title_of(r)));
    d.extend(toks(content_of(r)));
    d
}

/// BM25-lite relevance over the candidate set (df / idf computed across
/// candidates, k1/b fixed). Mirrors `rerank.py::bm25_lite`.
fn bm25_lite(rows: &[&SearxngResult], important: &HashSet<String>) -> Vec<f64> {
    let docs: Vec<Vec<String>> = rows.iter().map(|r| doc_tokens(r)).collect();
    let n = docs.len().max(1) as f64;
    let avgdl = docs.iter().map(|d| d.len()).sum::<usize>() as f64 / n;
    let mut df: HashMap<&str, usize> = HashMap::new();
    for d in &docs {
        let uniq: HashSet<&str> = d.iter().map(String::as_str).collect();
        for t in uniq {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    let n_docs = docs.len() as f64;
    docs.iter()
        .map(|d| {
            let dl = d.len() as f64;
            let mut rel = 0.0;
            for term in important {
                let tf = d.iter().filter(|t| t.as_str() == term.as_str()).count() as f64;
                if tf == 0.0 {
                    continue;
                }
                let dfi = *df.get(term.as_str()).unwrap_or(&0) as f64;
                let idf = (1.0 + (n_docs - dfi + 0.5) / (dfi + 0.5)).ln();
                rel += idf * (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * dl / avgdl.max(1.0)));
            }
            rel
        })
        .collect()
}

/// `true` if the row matches a junk signature. Mirrors `score.py::is_junk`.
fn is_junk(r: &SearxngResult) -> bool {
    let url = url_of(r);
    let d = domain(url);
    if JUNK_HOSTS.contains(d.as_str()) || JUNK_HOST_SUFFIXES.iter().any(|s| d.ends_with(s)) {
        return true;
    }
    let title = norm(title_of(r));
    // Dictionary / definition title pattern: a definition keyword in a short
    // (<= 6 token) title.
    let title_toks = toks(title_of(r));
    if title_toks.len() <= 6
        && [
            "definition",
            "meaning",
            "synonym",
            "synonyms",
            "antonym",
            "antonyms",
        ]
        .iter()
        .any(|kw| {
            title
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|w| w == *kw)
        })
    {
        return true;
    }
    // Bot-check / interstitial titles.
    for needle in [
        "just a moment",
        "attention required",
        "verify you are human",
        "are you a robot",
        "access denied",
        "enable javascript",
    ] {
        if title.contains(needle) {
            return true;
        }
    }
    // Asset-leak / non-content paths.
    let url_l = url.to_lowercase();
    if url_l.contains("/mapfiles/")
        || url_l.contains("/apple-app-site-association/")
        || url_l.contains("/.well-known/")
    {
        return true;
    }
    false
}

/// Important-term coverage guard. Mirrors `score.py::covers`.
fn covers(r: &SearxngResult, important: &HashSet<String>) -> bool {
    if important.is_empty() {
        return true;
    }
    let mut doc: HashSet<String> = toks(title_of(r)).into_iter().collect();
    doc.extend(toks(content_of(r)));
    let hit = important.iter().filter(|t| doc.contains(*t)).count();
    hit as f64 / important.len() as f64 >= MIN_COVERAGE
}

/// `true` if a competing-region token appears anywhere in the row.
/// Mirrors `score.py::geo_competing`.
fn geo_competing(r: &SearxngResult, competing: &[&str]) -> bool {
    if competing.is_empty() {
        return false;
    }
    let blob = norm(&format!("{} {} {}", title_of(r), content_of(r), url_of(r)));
    competing.iter().any(|c| blob.contains(c))
}

/// Geo signal: +1 for an in-region token, -1 for a competing token.
/// Mirrors `rerank.py::geo_score`.
fn geo_score(r: &SearxngResult, region: &[&str], competing: &[&str]) -> f64 {
    if region.is_empty() {
        return 0.0;
    }
    let blob = norm(&format!("{} {} {}", title_of(r), content_of(r), url_of(r)));
    let mut s = 0.0;
    if region.iter().any(|t| blob.contains(t)) {
        s += 1.0;
    }
    if !competing.is_empty() && competing.iter().any(|c| blob.contains(c)) {
        s -= 1.0;
    }
    s
}

/// Resolve the geo entry for a query, if any. Mirrors `score.py::geo_for`.
fn geo_for(query: &str) -> (&'static [&'static str], &'static [&'static str]) {
    let qn: HashSet<String> = toks(query).into_iter().collect();
    for (key, entry) in GEO.iter() {
        if qn.contains(*key) || (*key == "danang" && qn.contains("nang")) {
            return (entry.region, entry.competing);
        }
    }
    (&[], &[])
}

/// Important content terms of a query: tokens minus stopwords.
fn important_terms(query: &str) -> HashSet<String> {
    toks(query)
        .into_iter()
        .filter(|t| !STOPWORDS.contains(t.as_str()))
        .collect()
}

/// Run the full re-rank pipeline over raw SearXNG rows and return them ordered
/// best-first, deduped by registrable domain. Never returns empty unless
/// `rows` is empty (graceful degrade). Mirrors `rerank.py::rank_full` with the
/// junk filter always applied (including the degrade fallback).
pub fn rerank<'a>(rows: &'a [SearxngResult], query: &str) -> Vec<&'a SearxngResult> {
    if rows.is_empty() {
        return Vec::new();
    }
    let important = important_terms(query);
    let (region, competing) = geo_for(query);

    // STAGE2 junk filter is unconditional and survives the degrade fallback.
    let non_junk: Vec<&SearxngResult> = rows.iter().filter(|r| !is_junk(r)).collect();

    // STAGE3 coverage + geo-competing guards.
    let mut cands: Vec<&SearxngResult> = non_junk
        .iter()
        .copied()
        .filter(|r| covers(r, &important))
        .filter(|r| !geo_competing(r, competing))
        .collect();

    // DEGRADE: relax coverage / geo (but NOT junk). If even the non-junk pool
    // is empty (all rows were junk), fall back to the raw rows so we never
    // return empty on non-empty input.
    if cands.is_empty() {
        cands = if non_junk.is_empty() {
            rows.iter().collect()
        } else {
            non_junk
        };
    }

    // STAGE1/3b composite scoring.
    let rrf_vals: Vec<f64> = cands.iter().map(|r| rrf(r)).collect();
    let rrf_norm = minmax(&rrf_vals);
    let bm = bm25_lite(&cands, &important);
    let distinct_bm = bm.iter().map(|v| v.to_bits()).collect::<HashSet<_>>().len() > 1;
    let bm_norm = minmax(&bm);

    let mut scored: Vec<(f64, usize, &SearxngResult)> = cands
        .iter()
        .enumerate()
        .map(|(i, &r)| {
            let mut comp = W_RRF * rrf_norm(rrf_vals[i]);
            if distinct_bm {
                comp += W_REL * bm_norm(bm[i]);
            }
            comp += W_GEO * geo_score(r, region, competing);
            (comp, i, r)
        })
        .collect();

    // STAGE4 sort by composite desc, tie-break on original candidate index
    // (stable, mirrors the Python secondary key).
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });

    // STAGE4 dedupe by registrable domain, keep best per domain.
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<&SearxngResult> = Vec::with_capacity(scored.len());
    for (_, _, r) in scored {
        let rd = registrable(url_of(r));
        if !seen.insert(rd) {
            continue;
        }
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(url: &str, title: &str, content: &str, positions: Vec<u32>) -> SearxngResult {
        SearxngResult {
            url: Some(url.into()),
            title: Some(title.into()),
            engine: Some("test".into()),
            content: Some(content.into()),
            score: Some(1.0),
            engines: Vec::new(),
            positions,
            category: Some("general".into()),
            template: None,
            published_date: None,
            img_src: None,
            thumbnail_src: None,
            img_format: None,
            resolution: None,
        }
    }

    #[test]
    fn domain_strips_www_and_port() {
        assert_eq!(domain("https://www.Example.com:8080/path"), "example.com");
        assert_eq!(domain("http://sub.example.org/x"), "sub.example.org");
    }

    #[test]
    fn registrable_takes_last_two_labels() {
        assert_eq!(
            registrable("https://dictionary.cambridge.org/x"),
            "cambridge.org"
        );
        assert_eq!(
            registrable("https://www.tripadvisor.com/y"),
            "tripadvisor.com"
        );
    }

    #[test]
    fn junk_dictionary_host_dropped() {
        let r = row(
            "https://www.merriam-webster.com/dictionary/best",
            "best Definition",
            "",
            vec![1],
        );
        assert!(is_junk(&r));
    }

    #[test]
    fn junk_bot_check_title_dropped() {
        let r = row("https://example.com/", "Just a moment...", "", vec![1]);
        assert!(is_junk(&r));
    }

    #[test]
    fn non_junk_real_result_kept() {
        let r = row(
            "https://www.tripadvisor.com/Restaurants-Belgrade.html",
            "THE 10 BEST Restaurants in Belgrade",
            "best restaurants in belgrade serbia",
            vec![1],
        );
        assert!(!is_junk(&r));
    }

    #[test]
    fn dedupe_by_registrable_domain() {
        let rows = vec![
            row("https://a.com/1", "alpha beta", "alpha beta", vec![1]),
            row("https://a.com/2", "alpha beta", "alpha beta", vec![2]),
            row("https://b.com/1", "alpha beta", "alpha beta", vec![3]),
        ];
        let out = rerank(&rows, "alpha beta");
        let doms: Vec<String> = out.iter().map(|r| registrable(url_of(r))).collect();
        assert_eq!(doms, vec!["a.com", "b.com"]);
    }

    #[test]
    fn degrade_never_returns_empty_when_coverage_fails() {
        // No row covers the important terms, but they're not junk → degrade.
        let rows = vec![
            row("https://a.com/1", "unrelated", "nothing matches", vec![1]),
            row(
                "https://b.com/1",
                "also unrelated",
                "still nothing",
                vec![2],
            ),
        ];
        let out = rerank(&rows, "quantum chromodynamics lattice");
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn empty_input_returns_empty() {
        let rows: Vec<SearxngResult> = Vec::new();
        assert!(rerank(&rows, "anything").is_empty());
    }

    #[test]
    fn junk_never_leaks_through_degrade() {
        // All non-junk rows fail coverage; degrade must still drop junk.
        let rows = vec![
            row(
                "https://www.merriam-webster.com/dictionary/best",
                "best Definition",
                "best",
                vec![1],
            ),
            row("https://real.com/1", "unrelated", "no match here", vec![2]),
        ];
        let out = rerank(&rows, "quantum chromodynamics");
        assert!(out.iter().all(|r| !is_junk(r)));
        assert_eq!(out.len(), 1);
    }
}
