//! 汇率换算（frankfurter.dev），用于把不同计价货币的费用换算到预算货币。
//!
//! 汇率按 (from, to) 在进程内缓存；请求失败时使用常用货币兜底汇率并告警。

use std::collections::HashMap;
use std::sync::Mutex;

static FX_CACHE: Mutex<Option<HashMap<(String, String), f64>>> = Mutex::new(None);

/// 兜底汇率（网络失败时使用；覆盖常用 CNY/USD，其余按 1.0 并告警）。
fn fallback_rate(from: &str, to: &str) -> f64 {
    match (from, to) {
        ("CNY", "USD") => 0.14,
        ("USD", "CNY") => 7.2,
        _ => 1.0,
    }
}

fn cache_get(key: &(String, String)) -> Option<f64> {
    FX_CACHE
        .lock()
        .ok()?
        .as_ref()
        .and_then(|m| m.get(key).copied())
}

fn cache_put(key: (String, String), rate: f64) {
    if let Ok(mut g) = FX_CACHE.lock() {
        g.get_or_insert_with(HashMap::new).insert(key, rate);
    }
}

/// 从 frankfurter.dev 拉取汇率，容错解析多种返回结构。
async fn fetch_rate(from: &str, to: &str) -> Option<f64> {
    let url = format!("https://api.frankfurter.dev/v2/rate/{from}/{to}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    // 容错：{"rate": 7.2} / {"rates": {"CNY": 7.2}} / {"amount":1,"rates":{...}}
    if let Some(r) = v.get("rate").and_then(|x| x.as_f64()) {
        return Some(r);
    }
    if let Some(r) = v
        .get("rates")
        .and_then(|x| x.get(to))
        .and_then(|x| x.as_f64())
    {
        return Some(r);
    }
    None
}

/// 获取 from→to 汇率（同币种返回 1.0；带进程内缓存；失败用兜底值）。
pub async fn exchange_rate(from: &str, to: &str) -> f64 {
    let from = crate::pricing::normalize_currency(from);
    let to = crate::pricing::normalize_currency(to);
    if from == to {
        return 1.0;
    }
    let key = (from.clone(), to.clone());
    if let Some(r) = cache_get(&key) {
        return r;
    }
    let rate = match fetch_rate(&from, &to).await {
        Some(r) if r > 0.0 => r,
        _ => {
            let fb = fallback_rate(&from, &to);
            crate::term::println_err(&format!(
                "[fx] 获取 {from}/{to} 汇率失败，使用兜底汇率 {fb}"
            ));
            fb
        }
    };
    cache_put(key, rate);
    rate
}
