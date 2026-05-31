//! Corpus-level quality gate for the re-rank pipeline, run against the frozen
//! SearXNG / Tavily fixtures under `tests/fixtures/bench` (no network).
//!
//! Asserts the proven-prototype guarantees:
//!  - Junk@5 == 0 over the whole corpus (no dictionary / shopping / captcha
//!    host in any reranked top-5).
//!  - "top restaurants in belgrad" → Belgrade-Serbia travel/food domains, no
//!    competing-region tokens, no junk.
//!  - "python snake habitat" → reptile/animal domains, never python.org /
//!    codecademy (the programming-language homonym).
//!  - reranked mean CleanRel@5 materially beats the raw-score baseline.
//!
//! The metric helpers (`is_junk`, `covers`, `registrable`) intentionally
//! re-implement `tests/fixtures/bench/score.py` so the gate is independent of
//! the pipeline's own internals.

use std::collections::HashSet;

use crw_search::client::{SearxngResponse, SearxngResult};
use crw_search::rerank::rerank;

const STOPWORDS: &[&str] = &[
    "top", "best", "good", "greatest", "finest", "cheapest", "cheap", "the", "a", "an", "in", "of",
    "to", "for", "and", "or", "near", "how", "is", "are", "do", "does", "from", "with", "you",
    "your", "should", "per", "what", "2026", "2025",
];

const JUNK_HOSTS: &[&str] = &[
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
];

fn fold(c: char) -> char {
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

fn norm(s: &str) -> String {
    s.to_lowercase().chars().map(fold).collect()
}

fn toks(s: &str) -> Vec<String> {
    norm(s)
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn domain(url: &str) -> String {
    let host = url
        .split("//")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

fn registrable(url: &str) -> String {
    let d = domain(url);
    let parts: Vec<&str> = d.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        d
    }
}

fn is_junk(r: &SearxngResult) -> bool {
    let url = r.url.as_deref().unwrap_or("");
    let d = domain(url);
    if JUNK_HOSTS.contains(&d.as_str()) || d.ends_with("myshopify.com") {
        return true;
    }
    let title = r.title.as_deref().unwrap_or("");
    let tnorm = norm(title);
    let ttoks = toks(title);
    if ttoks.len() <= 6
        && [
            "definition",
            "meaning",
            "synonym",
            "synonyms",
            "antonym",
            "antonyms",
        ]
        .iter()
        .any(|kw| ttoks.iter().any(|w| w == kw))
    {
        return true;
    }
    for needle in [
        "just a moment",
        "attention required",
        "verify you are human",
        "are you a robot",
        "access denied",
        "enable javascript",
    ] {
        if tnorm.contains(needle) {
            return true;
        }
    }
    let ul = url.to_lowercase();
    ul.contains("/mapfiles/")
        || ul.contains("/apple-app-site-association/")
        || ul.contains("/.well-known/")
}

fn important_terms(query: &str) -> HashSet<String> {
    let stop: HashSet<&str> = STOPWORDS.iter().copied().collect();
    toks(query)
        .into_iter()
        .filter(|t| !stop.contains(t.as_str()))
        .collect()
}

fn covers(r: &SearxngResult, important: &HashSet<String>) -> bool {
    if important.is_empty() {
        return true;
    }
    let mut doc: HashSet<String> = toks(r.title.as_deref().unwrap_or("")).into_iter().collect();
    doc.extend(toks(r.content.as_deref().unwrap_or("")));
    let hit = important.iter().filter(|t| doc.contains(*t)).count();
    hit as f64 / important.len() as f64 >= 0.5
}

/// Baseline = current engine behavior: raw SearXNG score desc, dedupe by URL.
fn rank_baseline(rows: &[SearxngResult]) -> Vec<&SearxngResult> {
    let mut idx: Vec<&SearxngResult> = rows.iter().collect();
    idx.sort_by(|a, b| {
        b.score
            .unwrap_or(0.0)
            .partial_cmp(&a.score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for r in idx {
        let u = r.url.clone().unwrap_or_default();
        if seen.insert(u) {
            out.push(r);
        }
    }
    out
}

#[derive(serde::Deserialize)]
struct RawQuery {
    query: String,
    results: Vec<SearxngResult>,
}

fn bench_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bench")
}

fn load_corpus() -> Vec<RawQuery> {
    let p = bench_dir().join("searxng_raw.json");
    let raw = std::fs::read_to_string(&p).expect("read searxng_raw.json");
    serde_json::from_str(&raw).expect("parse searxng_raw.json")
}

fn resp(rows: Vec<SearxngResult>) -> SearxngResponse {
    SearxngResponse {
        results: rows,
        ..SearxngResponse::default()
    }
}

fn reranked_top5(q: &RawQuery) -> Vec<&SearxngResult> {
    rerank(&q.results, &q.query).into_iter().take(5).collect()
}

/// CleanRel@5 = fraction of top-5 that are non-junk AND cover important terms.
/// Normalized over a fixed window of 5 (matches `score.py`'s `/5`).
fn clean_rel_at5(top: &[&SearxngResult], important: &HashSet<String>) -> f64 {
    let n = top
        .iter()
        .filter(|r| !is_junk(r) && covers(r, important))
        .count();
    n as f64 / 5.0
}

#[test]
fn junk_at5_is_zero_over_corpus() {
    let corpus = load_corpus();
    let mut offenders = Vec::new();
    for q in &corpus {
        for r in reranked_top5(q) {
            if is_junk(r) {
                offenders.push((q.query.clone(), r.url.clone().unwrap_or_default()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "reranked top-5 leaked junk: {offenders:?}"
    );
}

#[test]
fn belgrad_restaurants_are_geo_correct_and_clean() {
    let corpus = load_corpus();
    let q = corpus
        .iter()
        .find(|q| q.query == "top restaurants in belgrad")
        .expect("belgrad query present");
    let top = reranked_top5(q);
    assert!(!top.is_empty());

    let expected_any = ["tripadvisor", "michelin", "lepetitchef", "travelinsighter"];
    let doms: Vec<String> = top
        .iter()
        .map(|r| registrable(r.url.as_deref().unwrap_or("")))
        .collect();
    assert!(
        doms.iter()
            .any(|d| expected_any.iter().any(|e| d.contains(e))),
        "expected a Belgrade travel/food domain in top-5, got: {doms:?}"
    );

    // No junk, no competing-region tokens (istanbul/maine/forest/...).
    let competing = ["istanbul", "maine", "montana", "turkey", "forest"];
    for r in &top {
        assert!(!is_junk(r), "junk in belgrad top-5: {:?}", r.url);
        let blob = norm(&format!(
            "{} {} {}",
            r.title.as_deref().unwrap_or(""),
            r.content.as_deref().unwrap_or(""),
            r.url.as_deref().unwrap_or("")
        ));
        for c in competing {
            assert!(
                !blob.contains(c),
                "competing-region token '{c}' in belgrad top-5: {:?}",
                r.url
            );
        }
    }
}

#[test]
fn python_snake_excludes_programming_homonym() {
    let corpus = load_corpus();
    let q = corpus
        .iter()
        .find(|q| q.query == "python snake habitat")
        .expect("python snake query present");
    let top = reranked_top5(q);
    assert!(!top.is_empty());

    let doms: Vec<String> = top
        .iter()
        .map(|r| registrable(r.url.as_deref().unwrap_or("")))
        .collect();
    for bad in ["python.org", "codecademy.com"] {
        assert!(
            !doms.iter().any(|d| d == bad),
            "homonym '{bad}' leaked into python-snake top-5: {doms:?}"
        );
    }
    // Should surface an animal / reptile reference domain.
    let animalish = [
        "petmd",
        "britannica",
        "nationalgeographic",
        "reptile",
        "animal",
        "smithsonian",
        "az-animals",
        "thoughtco",
    ];
    assert!(
        doms.iter().any(|d| animalish.iter().any(|a| d.contains(a)))
            || top.iter().any(|r| {
                let blob = norm(&format!(
                    "{} {}",
                    r.title.as_deref().unwrap_or(""),
                    r.content.as_deref().unwrap_or("")
                ));
                blob.contains("snake") || blob.contains("reptile") || blob.contains("habitat")
            }),
        "expected an animal/reptile source in python-snake top-5: {doms:?}"
    );
}

#[test]
fn reranked_cleanrel_beats_baseline() {
    let corpus = load_corpus();
    let mut base_sum = 0.0;
    let mut rerank_sum = 0.0;
    let mut base_junk = 0usize;
    for q in &corpus {
        let important = important_terms(&q.query);
        let base_top: Vec<&SearxngResult> = rank_baseline(&q.results).into_iter().take(5).collect();
        let rr_top = reranked_top5(q);
        base_sum += clean_rel_at5(&base_top, &important);
        rerank_sum += clean_rel_at5(&rr_top, &important);
        base_junk += base_top.iter().filter(|r| is_junk(r)).count();
    }
    let n = corpus.len() as f64;
    let base_mean = base_sum / n;
    let rerank_mean = rerank_sum / n;
    eprintln!(
        "CleanRel@5  baseline={base_mean:.3}  reranked={rerank_mean:.3}  (Δ={:.3}, baseline Junk@5 total={base_junk})",
        rerank_mean - base_mean
    );

    // Reranked must materially beat the baseline and clear a meaningful floor.
    // Numbers are bounded by snippet coverage in the frozen corpus; the proven
    // prototype lands at ~0.52 vs ~0.47 baseline with Junk@5 driven to 0.
    assert!(
        rerank_mean >= base_mean + 0.03,
        "reranked CleanRel@5 ({rerank_mean:.3}) must beat baseline ({base_mean:.3}) by >= 0.03"
    );
    assert!(
        rerank_mean >= 0.50,
        "reranked CleanRel@5 ({rerank_mean:.3}) below floor 0.50"
    );
    // The baseline leaks junk; the reranked path does not (asserted separately
    // in `junk_at5_is_zero_over_corpus`). Sanity-check the baseline is dirty so
    // this comparison is meaningful.
    assert!(base_junk > 0, "expected the raw baseline to leak junk");
}

#[test]
fn transform_flat_reranked_smoke() {
    // End-to-end through the public transform: a junk dictionary row must not
    // appear, a real travel row must.
    let rows = vec![
        SearxngResult {
            url: Some("https://www.merriam-webster.com/dictionary/best".into()),
            title: Some("best Definition & Meaning".into()),
            engine: Some("bing".into()),
            content: Some("the definition of best".into()),
            score: Some(1.0),
            engines: vec!["bing".into()],
            positions: vec![1],
            category: Some("general".into()),
            template: None,
            published_date: None,
            img_src: None,
            thumbnail_src: None,
            img_format: None,
            resolution: None,
        },
        SearxngResult {
            url: Some("https://www.tripadvisor.com/Restaurants-Belgrade.html".into()),
            title: Some("THE 10 BEST Restaurants in Belgrade".into()),
            engine: Some("duckduckgo".into()),
            content: Some("best restaurants in belgrade serbia".into()),
            score: Some(8.0),
            engines: vec!["google".into(), "duckduckgo".into()],
            positions: vec![1, 3],
            category: Some("general".into()),
            template: None,
            published_date: None,
            img_src: None,
            thumbnail_src: None,
            img_format: None,
            resolution: None,
        },
    ];
    let out = crw_search::transform_flat_reranked(&resp(rows), "best restaurants in belgrade", 5);
    assert_eq!(out.len(), 1, "junk dictionary row must be dropped");
    assert!(out[0].url.contains("tripadvisor"));
    assert_eq!(out[0].position, 1);
}
