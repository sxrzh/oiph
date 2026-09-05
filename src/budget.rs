//! API 费用预算：`~/.oiph/config/limit.json` 的 `limit_fee` 字段。
//!
//! ```json
//! { "limit_fee": { "limit": 100.0, "used": 0.0, "warn": 10.0, "currency": "CNY" } }
//! ```
//!
//! - `limit`：预算上限；`used`：已用（随使用累加，只增不减，按预算货币计）；
//!   `warn`：剩余额度低于该值时告警；`currency`：预算货币（ISO 代码，如 CNY/USD）。

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetFee {
    /// 预算上限
    pub limit: f64,
    /// 已用（预算货币）
    pub used: f64,
    /// 剩余低于该值时告警
    pub warn: f64,
    /// 预算货币（ISO 代码，如 CNY/USD）
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BudgetFile {
    limit_fee: BudgetFee,
}

pub fn path() -> PathBuf {
    crate::config::config_dir().join("limit.json")
}

impl BudgetFee {
    pub fn currency(&self) -> String {
        crate::pricing::normalize_currency(&self.currency)
    }

    #[allow(dead_code)]
    pub fn over_warn(&self) -> bool {
        self.limit - self.used < self.warn
    }

    fn write(&self) -> Result<()> {
        let p = path();
        std::fs::create_dir_all(crate::config::config_dir())?;
        std::fs::write(&p, serde_json::to_vec_pretty(&BudgetFile { limit_fee: self.clone() })?)
            .with_context(|| format!("写入 {} 失败", p.display()))?;
        Ok(())
    }
}

/// 读取预算；文件不存在表示预算功能未启用。
pub fn load() -> Option<BudgetFee> {
    let raw = std::fs::read_to_string(path()).ok()?;
    let file: BudgetFile = serde_json::from_str(&raw).ok()?;
    Some(file.limit_fee)
}

/// 保存（新建或更新，used 保留传入值）。
pub fn save(b: &BudgetFee) -> Result<()> {
    b.write()
}

/// 重置已用量（used = 0，limit/warn/currency 不变）。
pub fn reset_used() -> Result<BudgetFee> {
    let mut b = load().ok_or_else(|| {
        anyhow::anyhow!("未找到 {}（费用预算未配置）", path().display())
    })?;
    b.used = 0.0;
    save(&b)?;
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_over_warn() {
        let _guard = crate::paths::tests::lock_home();
        let home = std::env::temp_dir().join(format!("oiph_budget_{}", uuid::Uuid::new_v4()));
        #[allow(unused_unsafe)]
        unsafe { std::env::set_var("HOME", &home); }
        assert!(load().is_none(), "无 limit.json 时预算未启用");

        let b = BudgetFee { limit: 100.0, used: 95.0, warn: 10.0, currency: "CNY".into() };
        save(&b).unwrap();
        let loaded = load().unwrap();
        assert_eq!(loaded.limit, 100.0);
        assert!(loaded.over_warn());
        assert_eq!(loaded.currency(), "CNY");

        let b2 = reset_used().unwrap();
        assert_eq!(b2.used, 0.0);
        assert_eq!(b2.limit, 100.0);

        std::fs::remove_dir_all(&home).ok();
    }
}
