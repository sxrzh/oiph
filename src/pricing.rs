//! 模型计价：固定单价（agents.json 的 price）或 price-policy "auto" 自动识别供应商。
//!
//! auto 识别规则：按 base_url 子串匹配（base_url 为空时回退环境变量 OPENAI_BASE_URL）：
//! - 包含 "deepseek" → DeepSeek 峰谷计价（模型名子串匹配档位）
//! - 包含 "bigmodel" / "glm" → GLM（不做费用估算）
//! - 其他 → 不估算费用
//!
//! DeepSeek 高峰时段：北京时间周一至周五 9:00-12:00、14:00-18:00；
//! 空闲时段价格为高峰时段的一半。

use chrono::{Datelike, FixedOffset, Timelike, Utc};

use crate::client::ChatUsage;

/// 单价配置（元或美元 / M token）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceConfig {
    /// 输入价格/M token（缓存未命中）
    pub input: f64,
    /// 输入价格/M token（缓存命中）
    pub hit: f64,
    /// 输出价格/M token
    pub output: f64,
    /// 货币符号，例如 "￥" 和 "$"
    #[serde(default = "default_currency")]
    pub currency: String,
}

fn default_currency() -> String {
    "CNY".into()
}

/// 规范化货币代码：兼容旧配置中的符号（￥→CNY、$→USD），其余转大写。
pub fn normalize_currency(c: &str) -> String {
    match c.trim() {
        "￥" | "¥" | "RMB" | "rmb" => "CNY".into(),
        "$" | "usd" | "Usd" => "USD".into(),
        other => other.trim().to_uppercase(),
    }
}

/// 费用估算结果。
#[derive(Debug, Clone)]
pub struct Cost {
    pub currency: String,
    pub amount: f64,
}

/// 计价策略。
#[derive(Debug, Clone, Default)]
pub enum Pricing {
    /// 不估算费用
    #[default]
    None,
    /// 固定单价
    Fixed(PriceConfig),
    /// DeepSeek 峰谷计价
    DeepSeek,
}

/// 模型档位费率（高峰时段，￥/M token）。
struct ModelRates {
    hit: f64,
    miss: f64,
    output: f64,
}

/// 高峰时段（北京时间周一至周五 9:00-12:00、14:00-18:00）。
fn is_peak_beijing(t: chrono::DateTime<FixedOffset>) -> bool {
    use chrono::Weekday::*;
    let wd = t.weekday();
    let hour = t.hour();
    let in_morning = (9..12).contains(&hour);
    let in_afternoon = (14..18).contains(&hour);
    matches!(wd, Mon | Tue | Wed | Thu | Fri) && (in_morning || in_afternoon)
}

/// 当前北京时间是否高峰时段。
pub fn is_peak_now() -> bool {
    let bj = FixedOffset::east_opt(8 * 3600).expect("UTC+8");
    let now = Utc::now().with_timezone(&bj);
    is_peak_beijing(now)
}

/// 按模型名子串匹配 DeepSeek 档位费率。
fn deepseek_rates(model: &str) -> ModelRates {
    let m = model.to_lowercase();
    // 注意顺序：vision-exp 是 flash 的超集子串，需先匹配
    if m.contains("vision-exp") {
        ModelRates { hit: 0.10, miss: 3.0, output: 9.0 }
    } else if m.contains("pro") {
        ModelRates { hit: 0.30, miss: 9.0, output: 27.0 }
    } else {
        // flash 及未知 deepseek 模型按 flash 档
        ModelRates { hit: 0.10, miss: 3.0, output: 9.0 }
    }
}

/// price-policy "auto"：按 base_url 子串识别供应商（base_url 为空时回退
/// 环境变量 OPENAI_BASE_URL）。
pub fn auto(base_url: &str) -> Pricing {
    let url = base_url.to_lowercase();
    let url = if url.trim().is_empty() {
        std::env::var("OPENAI_BASE_URL").unwrap_or_default().to_lowercase()
    } else {
        url
    };
    if url.contains("deepseek") {
        Pricing::DeepSeek
    } else if url.contains("bigmodel") || url.contains("glm") {
        // GLM 不需要估算价格
        Pricing::None
    } else {
        Pricing::None
    }
}

impl Pricing {
    pub fn fixed(p: PriceConfig) -> Self {
        Pricing::Fixed(p)
    }

    /// 估算一次调用序列的费用。无法计价时返回 None。
    pub fn estimate(&self, usage: &ChatUsage, model: &str) -> Option<Cost> {
        match self {
            Pricing::None => None,
            Pricing::Fixed(p) => {
                let hit = usage.cache_hit_tokens.unwrap_or(0) as f64;
                let miss = usage
                    .cache_miss_tokens
                    .unwrap_or(usage.prompt_tokens.saturating_sub(usage.cache_hit_tokens.unwrap_or(0)))
                    as f64;
                let amount = (miss * p.input + hit * p.hit + usage.completion_tokens as f64 * p.output) / 1e6;
                Some(Cost { currency: normalize_currency(&p.currency), amount })
            }
            Pricing::DeepSeek => {
                let r = deepseek_rates(model);
                let factor = if is_peak_now() { 1.0 } else { 0.5 };
                let hit = usage.cache_hit_tokens.unwrap_or(0) as f64;
                let miss = usage
                    .cache_miss_tokens
                    .unwrap_or(usage.prompt_tokens.saturating_sub(usage.cache_hit_tokens.unwrap_or(0)))
                    as f64;
                let amount =
                    (miss * r.miss + hit * r.hit + usage.completion_tokens as f64 * r.output) * factor / 1e6;
                Some(Cost { currency: "CNY".into(), amount })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bj_ymd(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<FixedOffset> {
        use chrono::TimeZone;
        let bj = FixedOffset::east_opt(8 * 3600).unwrap();
        bj.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn peak_hours() {
        // 周一 9:00-12:00、14:00-18:00 为高峰
        assert!(is_peak_beijing(bj_ymd(2026, 9, 7, 9, 0)));
        assert!(is_peak_beijing(bj_ymd(2026, 9, 7, 10, 30)));
        assert!(is_peak_beijing(bj_ymd(2026, 9, 7, 11, 59)));
        assert!(is_peak_beijing(bj_ymd(2026, 9, 7, 14, 0)));
        assert!(is_peak_beijing(bj_ymd(2026, 9, 7, 17, 59)));
        // 午休与晚间、周末为空闲
        assert!(!is_peak_beijing(bj_ymd(2026, 9, 7, 12, 0)));
        assert!(!is_peak_beijing(bj_ymd(2026, 9, 7, 13, 59)));
        assert!(!is_peak_beijing(bj_ymd(2026, 9, 7, 18, 0)));
        assert!(!is_peak_beijing(bj_ymd(2026, 9, 7, 8, 59)));
        assert!(!is_peak_beijing(bj_ymd(2026, 9, 6, 10, 0))); // 周日
        assert!(!is_peak_beijing(bj_ymd(2026, 9, 12, 10, 0))); // 周六
    }

    #[test]
    fn deepseek_valley_is_half() {
        let usage = ChatUsage {
            prompt_tokens: 1_000_000,
            completion_tokens: 1_000_000,
            total_tokens: 2_000_000,
            cache_hit_tokens: None,
            cache_miss_tokens: None,
        };
        let p = Pricing::DeepSeek;
        // 无法直接控制 is_peak_now，全量（1M miss + 1M out）费用应落在
        // flash 高峰 12￥ 与空闲 6￥ 之间（档位匹配为 flash 系）
        if let Some(c) = p.estimate(&usage, "deepseek-v4-flash") {
            assert!(c.amount > 5.999 && c.amount < 12.001, "got {}", c.amount);
        }
    }

    #[test]
    fn deepseek_fixed_cost_math() {
        // 直接验证档位与计算：flash 空闲 = 高峰一半
        let r = deepseek_rates("deepseek-v4-flash");
        assert_eq!(r.miss, 3.0);
        let rv = deepseek_rates("deepseek-v4-flash-vision-exp");
        assert_eq!(rv.miss, 3.0);
        let rp = deepseek_rates("DeepSeek-V4-Pro");
        assert_eq!(rp.miss, 9.0);
        let r_unknown = deepseek_rates("deepseek-r1");
        assert_eq!(r_unknown.miss, 3.0);
    }

    #[test]
    fn fixed_price_math() {
        let p = Pricing::fixed(PriceConfig {
            input: 2.0,
            hit: 0.2,
            output: 8.0,
            currency: "$".into(),
        });
        let usage = ChatUsage {
            prompt_tokens: 1_000_000,
            completion_tokens: 500_000,
            total_tokens: 1_500_000,
            cache_hit_tokens: Some(400_000),
            cache_miss_tokens: None,
        };
        let c = p.estimate(&usage, "m").unwrap();
        // miss 600k*2 + hit 400k*0.2 + out 500k*8 = 1200 + 80 + 4000 = 5280 / 1e6*1e6
        assert!((c.amount - 5.28).abs() < 1e-9);
        assert_eq!(c.currency, "USD");
    }

    #[test]
    fn auto_detect() {
        assert!(matches!(auto("https://api.deepseek.com/v1"), Pricing::DeepSeek));
        assert!(matches!(auto("https://open.bigmodel.cn/api/paas/v4"), Pricing::None));
        assert!(matches!(auto("https://api.openai.com/v1"), Pricing::None));
    }

    #[test]
    fn normalize_currency_codes() {
        assert_eq!(normalize_currency("￥"), "CNY");
        assert_eq!(normalize_currency("¥"), "CNY");
        assert_eq!(normalize_currency("$"), "USD");
        assert_eq!(normalize_currency("cny"), "CNY");
        assert_eq!(normalize_currency("USD"), "USD");
        assert_eq!(normalize_currency("EUR"), "EUR");
    }
}
