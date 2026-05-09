//! Transform a [`SearxngResponse`] into the public flat / grouped result
//! shapes. Direct port of `crw-saas/src/lib/search-transform.ts`.

use std::collections::HashSet;

use crw_core::types::{GroupedSearchData, ImageResult, SearchResult, SearchSource};

use crate::client::{SearxngResponse, SearxngResult};

fn score_or_zero(r: &SearxngResult) -> f64 {
    r.score.unwrap_or(0.0)
}

/// Stable-sorted by descending `score` (missing scores treated as 0).
fn sort_by_score(items: &mut [SearxngResult]) {
    items.sort_by(|a, b| {
        score_or_zero(b)
            .partial_cmp(&score_or_zero(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn dedupe_by_url(items: Vec<SearxngResult>) -> Vec<SearxngResult> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if seen.insert(item.url.clone()) {
            out.push(item);
        }
    }
    out
}

fn to_search_result(r: &SearxngResult, position: u32) -> SearchResult {
    SearchResult {
        url: r.url.clone(),
        title: r.title.clone(),
        description: r.content.clone().unwrap_or_default(),
        position,
        score: r.score,
        published_date: r.published_date.clone(),
        category: r.category.clone(),
        markdown: None,
        html: None,
        raw_html: None,
        links: None,
        metadata: None,
    }
}

fn to_image_result(r: &SearxngResult, position: u32) -> ImageResult {
    ImageResult {
        url: r.url.clone(),
        title: r.title.clone(),
        description: r.content.clone().unwrap_or_default(),
        image_url: r.img_src.clone().unwrap_or_else(|| r.url.clone()),
        position,
        thumbnail_url: r.thumbnail_src.clone(),
        image_format: r.img_format.clone(),
        resolution: r.resolution.clone(),
    }
}

/// Flat output: dedupe by URL, sort by score, slice to `limit`.
///
/// Note: SaaS sorts then dedupes, so a higher-scored duplicate wins. We
/// preserve that order — see `crw-saas/src/lib/search-transform.ts:73`.
pub fn transform_flat(response: &SearxngResponse, limit: u32) -> Vec<SearchResult> {
    let mut results = response.results.clone();
    sort_by_score(&mut results);
    let deduped = dedupe_by_url(results);
    deduped
        .into_iter()
        .take(limit as usize)
        .enumerate()
        .map(|(i, r)| to_search_result(&r, (i + 1) as u32))
        .collect()
}

/// Grouped output: filter by `sources`, then per-bucket sort/dedupe/slice.
/// Limit applies **per source**, not in total — matches SaaS semantics.
pub fn transform_grouped(
    response: &SearxngResponse,
    sources: &[SearchSource],
    limit: u32,
) -> GroupedSearchData {
    let mut data = GroupedSearchData::default();
    let cap = limit as usize;

    if sources.contains(&SearchSource::Web) {
        let filtered: Vec<SearxngResult> = response
            .results
            .iter()
            .filter(|r| {
                let cat = r.category.as_deref();
                cat == Some("general") || (r.img_src.is_none() && cat != Some("news"))
            })
            .cloned()
            .collect();
        let mut sorted = filtered;
        sort_by_score(&mut sorted);
        let deduped = dedupe_by_url(sorted);
        data.web = Some(
            deduped
                .into_iter()
                .take(cap)
                .enumerate()
                .map(|(i, r)| to_search_result(&r, (i + 1) as u32))
                .collect(),
        );
    }

    if sources.contains(&SearchSource::News) {
        let filtered: Vec<SearxngResult> = response
            .results
            .iter()
            .filter(|r| r.category.as_deref() == Some("news"))
            .cloned()
            .collect();
        let mut sorted = filtered;
        sort_by_score(&mut sorted);
        let deduped = dedupe_by_url(sorted);
        data.news = Some(
            deduped
                .into_iter()
                .take(cap)
                .enumerate()
                .map(|(i, r)| to_search_result(&r, (i + 1) as u32))
                .collect(),
        );
    }

    if sources.contains(&SearchSource::Images) {
        let filtered: Vec<SearxngResult> = response
            .results
            .iter()
            .filter(|r| r.category.as_deref() == Some("images") || r.img_src.is_some())
            .cloned()
            .collect();
        let mut sorted = filtered;
        sort_by_score(&mut sorted);
        let deduped = dedupe_by_url(sorted);
        data.images = Some(
            deduped
                .into_iter()
                .take(cap)
                .enumerate()
                .map(|(i, r)| to_image_result(&r, (i + 1) as u32))
                .collect(),
        );
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(url: &str, score: f64, content: &str) -> SearxngResult {
        SearxngResult {
            url: url.into(),
            title: format!("title-{url}"),
            engine: "test".into(),
            content: Some(content.into()),
            score: Some(score),
            category: Some("general".into()),
            template: None,
            published_date: None,
            img_src: None,
            thumbnail_src: None,
            img_format: None,
            resolution: None,
        }
    }

    fn news(url: &str, score: f64) -> SearxngResult {
        SearxngResult {
            url: url.into(),
            title: format!("news-{url}"),
            engine: "test".into(),
            content: Some("snippet".into()),
            score: Some(score),
            category: Some("news".into()),
            template: None,
            published_date: Some("2026-05-01T00:00:00Z".into()),
            img_src: None,
            thumbnail_src: None,
            img_format: None,
            resolution: None,
        }
    }

    fn image(url: &str, score: f64, img: &str) -> SearxngResult {
        SearxngResult {
            url: url.into(),
            title: format!("img-{url}"),
            engine: "test".into(),
            content: Some(String::new()),
            score: Some(score),
            category: Some("images".into()),
            template: Some("images.html".into()),
            published_date: None,
            img_src: Some(img.into()),
            thumbnail_src: Some(format!("{img}.thumb")),
            img_format: Some("jpeg".into()),
            resolution: Some("1920x1080".into()),
        }
    }

    fn resp(items: Vec<SearxngResult>) -> SearxngResponse {
        SearxngResponse {
            results: items,
            ..SearxngResponse::default()
        }
    }

    #[test]
    fn flat_sorts_by_score_desc() {
        let res = transform_flat(
            &resp(vec![r("a", 0.1, "A"), r("b", 0.9, "B"), r("c", 0.5, "C")]),
            5,
        );
        assert_eq!(
            res.iter().map(|x| x.url.as_str()).collect::<Vec<_>>(),
            vec!["b", "c", "a"]
        );
        assert_eq!(res[0].position, 1);
        assert_eq!(res[1].position, 2);
        assert_eq!(res[2].position, 3);
    }

    #[test]
    fn flat_dedupe_keeps_highest_score() {
        let res = transform_flat(&resp(vec![r("a", 0.1, "low"), r("a", 0.9, "high")]), 5);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].description, "high");
    }

    #[test]
    fn flat_respects_limit() {
        let res = transform_flat(
            &resp(vec![r("a", 0.9, "A"), r("b", 0.8, "B"), r("c", 0.7, "C")]),
            2,
        );
        assert_eq!(res.len(), 2);
    }

    #[test]
    fn flat_missing_score_treated_as_zero() {
        let mut a = r("a", 0.0, "A");
        a.score = None;
        let res = transform_flat(&resp(vec![a, r("b", 0.5, "B")]), 5);
        assert_eq!(res[0].url, "b");
    }

    #[test]
    fn grouped_web_filters_general_and_unknown() {
        let res = transform_grouped(
            &resp(vec![
                r("g", 0.9, ""),
                news("n", 0.8),
                image("i", 0.7, "https://i.img"),
            ]),
            &[SearchSource::Web],
            5,
        );
        let web = res.web.unwrap();
        assert_eq!(
            web.iter().map(|x| x.url.as_str()).collect::<Vec<_>>(),
            vec!["g"]
        );
    }

    #[test]
    fn grouped_news_only_news_category() {
        let res = transform_grouped(
            &resp(vec![r("g", 0.9, ""), news("n1", 0.8), news("n2", 0.6)]),
            &[SearchSource::News],
            5,
        );
        let n = res.news.unwrap();
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].url, "n1");
        assert!(n[0].published_date.is_some());
    }

    #[test]
    fn grouped_images_picks_image_or_img_src() {
        let mut general_with_img = r("g", 0.5, "");
        general_with_img.img_src = Some("https://x.png".into());

        let res = transform_grouped(
            &resp(vec![image("i", 0.9, "https://i.img"), general_with_img]),
            &[SearchSource::Images],
            5,
        );
        let imgs = res.images.unwrap();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].url, "i");
        assert_eq!(imgs[0].image_url, "https://i.img");
    }

    #[test]
    fn grouped_image_falls_back_to_url_when_img_src_missing() {
        let mut img = image("i", 0.9, "");
        img.img_src = None; // category=images but no img_src
        let res = transform_grouped(&resp(vec![img]), &[SearchSource::Images], 5);
        let imgs = res.images.unwrap();
        assert_eq!(imgs[0].image_url, "i"); // falls back to url
    }

    #[test]
    fn grouped_limit_applies_per_source() {
        let mut items = vec![];
        for i in 0..5 {
            items.push(r(&format!("g{i}"), 1.0 - i as f64 * 0.1, ""));
            items.push(news(&format!("n{i}"), 1.0 - i as f64 * 0.1));
        }
        let res = transform_grouped(&resp(items), &[SearchSource::Web, SearchSource::News], 2);
        assert_eq!(res.web.unwrap().len(), 2);
        assert_eq!(res.news.unwrap().len(), 2);
    }

    #[test]
    fn grouped_unrequested_source_omitted() {
        let res = transform_grouped(&resp(vec![r("g", 0.9, "")]), &[SearchSource::Web], 5);
        assert!(res.web.is_some());
        assert!(res.news.is_none());
        assert!(res.images.is_none());
    }
}
