//! 原题查找：支持 cpret.online（默认）与 yuantiji.ac 两个后端。
//!
//! 内置频率控制：两次请求之间至少间隔一定时间，避免给站点造成压力。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use scraper::{Html, Selector};
use serde::Deserialize;

const CPRET_API_URL: &str = "https://cpret.online/api/search";
const YUANTIJII_API_URL: &str = "https://yuantiji.ac/api/search";

/// 两次请求间的最小间隔。
const MIN_INTERVAL: Duration = Duration::from_secs(3);

/// 上一次请求的时间戳，用于频率控制（所有后端共享）。
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    #[default]
    Cpret,
    Yuantiji,
}

impl Backend {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "cpret" | "cpret.online" => Ok(Self::Cpret),
            "yuantiji" | "yuantiji.ac" => Ok(Self::Yuantiji),
            other => Err(anyhow!("未知查重后端 '{other}'（可用：cpret, yuantiji）")),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cpret => "cpret",
            Self::Yuantiji => "yuantiji",
        }
    }
}

/// 单条搜索结果。
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    #[allow(dead_code)]
    pub uid: String,
    pub title: String,
    pub src: String,
    pub url: String,
    pub cos: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub original: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub t0: Option<String>,
}

/// 等待直到满足频率限制。
async fn rate_limit() {
    let wait = {
        let guard = LAST_REQUEST.lock().unwrap();
        match *guard {
            Some(last) => {
                let elapsed = last.elapsed();
                if elapsed < MIN_INTERVAL {
                    Some(MIN_INTERVAL - elapsed)
                } else {
                    None
                }
            }
            None => None,
        }
    };
    if let Some(d) = wait {
        eprintln!("[dupcheck] 频率限制：等待 {:.1}s", d.as_secs_f64());
        tokio::time::sleep(d).await;
    }
    *LAST_REQUEST.lock().unwrap() = Some(Instant::now());
}

/// 统一搜索入口：按后端分发。
pub async fn search(query: &str, k: Option<u64>, backend: Backend) -> Result<Vec<SearchResult>> {
    match backend {
        Backend::Cpret => search_cpret(query, k).await,
        Backend::Yuantiji => search_yuantiji(query, k).await,
    }
}

/// 判断是否为疑似原题：首条结果相似度 ≥ 0.85。
pub fn is_likely_duplicate(results: &[SearchResult]) -> bool {
    results
        .first()
        .is_some_and(|r| r.cos >= 0.85)
}

// ---------------------------------------------------------------------------
// cpret.online
// ---------------------------------------------------------------------------

const OJ_LIST: &[&str] = &[
    "AIZU",
    "AtCoder",
    "BZOJ",
    "CodeChef",
    "Codeforces",
    "CodeforcesGym",
    "Hydro",
    "LeetCode",
    "LibreOJ",
    "Luogu",
    "Nowcoder",
    "QOJ",
    "SPOJ",
    "UOJ",
];

/// cpret.online 返回 JSON，其中 `html` 字段是渲染好的 HTML 列表。
#[derive(Deserialize)]
struct CpretResponse {
    #[serde(default)]
    html: String,
}

async fn search_cpret(query: &str, k: Option<u64>) -> Result<Vec<SearchResult>> {
    rate_limit().await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("构造 HTTP 客户端失败")?;

    let mut params: Vec<(&str, &str)> = vec![
        ("lang", "zh"),
        ("q", query),
        ("page", "1"),
    ];
    for oj in OJ_LIST {
        params.push(("oj", oj));
    }

    let resp = client
        .get(CPRET_API_URL)
        .header("accept", "application/json")
        .header("accept-language", "zh-CN,zh;q=0.9,en;q=0.6")
        .header("referer", "https://cpret.online/")
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36",
        )
        .query(&params)
        .send()
        .await
        .context("请求 cpret.online 失败")?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!(
            "cpret.online 返回 HTTP {status}：\n{}",
            &text[..text.len().min(500)]
        );
    }

    let parsed: CpretResponse =
        serde_json::from_str(&text).context("解析 cpret.online 响应失败")?;

    let results = parse_cpret_html(&parsed.html);
    let limit = k.unwrap_or(20) as usize;
    Ok(results.into_iter().take(limit).collect())
}

/// 从 cpret 返回的 HTML 列表中提取搜索结果。
fn parse_cpret_html(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let li_sel = Selector::parse("li.list-group-item").unwrap();
    let link_sel = Selector::parse(r#"a[target="_blank"]"#).unwrap();
    let oj_sel = Selector::parse("small").unwrap();
    let score_sel = Selector::parse("span.score-badge").unwrap();

    let mut results = Vec::new();
    for (i, li) in document.select(&li_sel).enumerate() {
        let Some(a) = li.select(&link_sel).next() else {
            continue;
        };
        let title = a.text().collect::<Vec<_>>().join("").trim().to_string();
        if title.is_empty() {
            continue;
        }
        let url = a.value().attr("href").unwrap_or("").to_string();
        let src = li
            .select(&oj_sel)
            .next()
            .map(|e| e.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();
        let cos = li
            .select(&score_sel)
            .next()
            .and_then(|e| e.text().collect::<Vec<_>>().join("").trim().parse::<f64>().ok())
            .unwrap_or(0.0);

        results.push(SearchResult {
            uid: format!("cpret/{}", i + 1),
            title,
            src,
            url,
            cos,
            original: None,
            t0: None,
        });
    }
    results
}

// ---------------------------------------------------------------------------
// yuantiji.ac
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct YuantijiResponse {
    #[serde(default)]
    results: Vec<YuantijiResult>,
}

#[derive(Debug, Deserialize)]
struct YuantijiResult {
    #[allow(dead_code)]
    uid: String,
    title: String,
    src: String,
    url: String,
    cos: f64,
    #[serde(default)]
    #[allow(dead_code)]
    original: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    t0: Option<String>,
}

async fn search_yuantiji(query: &str, k: Option<u64>) -> Result<Vec<SearchResult>> {
    rate_limit().await;

    let k_val = k.unwrap_or(20);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("构造 HTTP 客户端失败")?;

    let body = serde_json::json!({
        "query": query,
        "k": k_val,
        "rewrite": true,
        "skip_short": true,
        "rerank": false,
    });

    let resp = client
        .post(YUANTIJII_API_URL)
        .header("accept", "*/*")
        .header("accept-language", "zh-CN,zh;q=0.9,en;q=0.6")
        .header("content-type", "application/json")
        .header("origin", "https://yuantiji.ac")
        .header("referer", "https://yuantiji.ac/")
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36",
        )
        .json(&body)
        .send()
        .await
        .context("请求 yuantiji.ac 失败")?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!(
            "yuantiji.ac 返回 HTTP {status}：\n{}",
            &text[..text.len().min(500)]
        );
    }

    let parsed: YuantijiResponse = serde_json::from_str(&text)
        .context("解析 yuantiji.ac 响应失败")?;
    Ok(parsed
        .results
        .into_iter()
        .map(|r| SearchResult {
            uid: r.uid,
            title: r.title,
            src: r.src,
            url: r.url,
            cos: r.cos,
            original: r.original,
            t0: r.t0,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HTML: &str = r#"
<ul class="list-group">
  <li class="list-group-item">
    <div class="d-flex justify-content-between">
      <div>
        <span class="badge bg-secondary me-2">#1</span>
        <a href="https://qoj.ac/problem/1701" target="_blank">Exercise Route</a>
        <small class="text-muted ms-2">QOJ</small>
      </div>
      <span class="score-badge text-muted">0.9234</span>
    </div>
  </li>
  <li class="list-group-item">
    <div class="d-flex justify-content-between">
      <div>
        <span class="badge bg-secondary me-2">#2</span>
        <a href="https://example.com/p/42" target="_blank">Some Problem</a>
        <small class="text-muted ms-2">Codeforces</small>
      </div>
      <span class="score-badge text-muted">0.7000</span>
    </div>
  </li>
</ul>"#;

    #[test]
    fn parse_cpret_html_extracts_results() {
        let results = parse_cpret_html(SAMPLE_HTML);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Exercise Route");
        assert_eq!(results[0].url, "https://qoj.ac/problem/1701");
        assert_eq!(results[0].src, "QOJ");
        assert!((results[0].cos - 0.9234).abs() < 1e-6);
        assert_eq!(results[1].title, "Some Problem");
        assert_eq!(results[1].src, "Codeforces");
    }

    #[test]
    fn parse_cpret_html_empty() {
        let results = parse_cpret_html("<ul></ul>");
        assert!(results.is_empty());
    }

    #[test]
    fn backend_parse() {
        assert_eq!(Backend::parse("cpret").unwrap(), Backend::Cpret);
        assert_eq!(Backend::parse("yuantiji").unwrap(), Backend::Yuantiji);
        assert_eq!(Backend::parse("Cpret.Online").unwrap(), Backend::Cpret);
        assert!(Backend::parse("nope").is_err());
        assert_eq!(Backend::default(), Backend::Cpret);
    }

    #[test]
    fn is_likely_duplicate_threshold() {
        let r = SearchResult {
            uid: "a".into(), title: "t".into(), src: "s".into(),
            url: "u".into(), cos: 0.9, original: None, t0: None,
        };
        assert!(is_likely_duplicate(&[r.clone()]));
        let r2 = SearchResult { cos: 0.8, ..r };
        assert!(!is_likely_duplicate(&[r2]));
        assert!(!is_likely_duplicate(&[]));
    }

    #[test]
    fn yuantiji_response_parse() {
        let json = r#"{"results":[{"uid":"X/1","title":"T","src":"S","url":"U","cos":0.95,"original":"o","t0":"w"}]}"#;
        let resp: YuantijiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].title, "T");
        assert!((resp.results[0].cos - 0.95).abs() < 1e-6);
    }
}
