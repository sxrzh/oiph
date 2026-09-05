//! 全局 agent 配置：`~/.oiph/config/agents.json` + 提示词文件。
//!
//! agents.json 结构（每个 agent 一项）：
//! ```json
//! {
//!   "supervisor": { "base_url": null, "api_key": null,
//!                   "prompt": "~/.oiph/config/prompts/supervisor.md" },
//!   ...
//! }
//! ```
//! - `base_url` / `api_key` 为 null 时回退到全局命令行参数
//! - `prompt` 为提示词文件路径（支持 `~` 展开）
//! - `reasoning`：是否开启思考模式（缺省不发送该参数）
//! - `price`：固定单价 { input, hit, output, currency }（单位：货币/M token）
//! - `price-policy`：目前仅支持 "auto"（按 base_url 识别供应商自动计价，
//!   支持 DeepSeek 峰谷计价；GLM 不估算费用）；`price` 与 `price-policy`
//!   都没有时同样使用 auto 模式
//! - `max_context`：最长上下文长度（token 估算），超过则先压缩再回传；
//!   缺省 1048576
//! - 另有可选的 "compactor" 项：上下文压缩模型；缺省回退 supervisor 的
//!   客户端与内置压缩提示词

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::client::Client;
use crate::pricing::{PriceConfig, Pricing};
use crate::prompts::AgentPrompts;

pub const AGENTS: &[&str] = &["supervisor", "statement", "solution", "auxiliary", "searching"];

/// 上下文压缩 agent（可选配置项）。
pub const COMPACTOR: &str = "compactor";

/// 未配置 max_context 时的默认最长上下文。
pub const DEFAULT_MAX_CONTEXT: u64 = 1_048_576;

/// compactor 未配置提示词文件时的内置默认压缩提示词。
pub const DEFAULT_COMPACTOR_PROMPT: &str = "以上是本次会话此前的全部对话。\
请提炼出会话意图、当前状态、关键决策及理由、待办任务、关键背景，\
形成简洁但完整的摘要，供后续在同一上下文中继续工作使用。\
保留所有关键信息（文件路径、题目 id、参数、结论等），直接输出摘要内容。";

pub fn config_dir() -> PathBuf {
    crate::paths::oiph_home().join("config")
}

pub fn agents_config_path() -> PathBuf {
    config_dir().join("agents.json")
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    /// 提示词文件路径；compactor 可省略（使用内置默认提示词）。
    #[serde(default)]
    pub prompt: Option<String>,
    /// 是否开启思考模式（缺省不发送该参数）。
    #[serde(default)]
    pub reasoning: Option<bool>,
    /// 固定单价（优先于 price-policy）。
    #[serde(default)]
    pub price: Option<PriceConfig>,
    /// 计价策略，目前仅支持 "auto"；缺省（与 price 均未设置）即 auto。
    #[serde(default, rename = "price-policy")]
    pub price_policy: Option<String>,
    /// 最长上下文长度（token 估算），超过则先压缩再回传。
    #[serde(default)]
    pub max_context: Option<u64>,
}

pub type AgentsConfig = HashMap<String, AgentConfig>;

/// agent 运行设置（从 agents.json 解析）。
#[derive(Debug, Clone)]
pub struct AgentSettings {
    /// 思考模式。
    pub reasoning: Option<bool>,
    /// 最长上下文（token 估算）。
    pub max_context: u64,
    /// 计价策略。
    pub pricing: Pricing,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            reasoning: None,
            max_context: DEFAULT_MAX_CONTEXT,
            pricing: Pricing::None,
        }
    }
}

/// 展开 `~` 前缀。
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest);
        }
    PathBuf::from(path)
}

/// 加载 agents.json。文件不存在时报错（提示运行 init.sh）。
pub fn load_agents_config() -> Result<AgentsConfig> {
    let path = agents_config_path();
    if !path.is_file() {
        bail!(
            "未找到 {}，请先运行 init.sh 初始化",
            path.display()
        );
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读取 {}", path.display()))?;
    let cfg: AgentsConfig = serde_json::from_str(&raw)
        .with_context(|| format!("解析 {}", path.display()))?;
    for name in AGENTS {
        if !cfg.contains_key(*name) {
            bail!("{} 缺少 agent '{name}' 的配置", path.display());
        }
    }
    Ok(cfg)
}

/// 启动时加载的全部 agent 设置：提示词 + 每个 agent 的独立客户端 + 运行设置。
pub struct AgentSetup {
    pub prompts: AgentPrompts,
    pub clients: HashMap<String, Client>,
    pub settings: HashMap<String, AgentSettings>,
    /// compactor 的压缩提示词（未配置时为内置默认）。
    pub compactor_prompt: String,
}

/// 构建单个 agent 的计价策略。
fn build_pricing(ac: &AgentConfig, global_base_url: &str) -> Result<Pricing> {
    if let Some(p) = &ac.price {
        return Ok(Pricing::fixed(p.clone()));
    }
    if let Some(policy) = &ac.price_policy
        && policy != "auto" {
            bail!("不支持的 price-policy：'{policy}'（目前仅支持 \"auto\"）");
        }
    // price 与 price-policy 都没有 → auto 模式
    let base = ac.base_url.clone().unwrap_or_else(|| global_base_url.to_string());
    Ok(crate::pricing::auto(&base))
}

/// 加载提示词文件并构建 per-agent 客户端与运行设置。
pub fn load_agent_setup(
    cfg: &AgentsConfig,
    global_base_url: &str,
    global_api_key: &str,
) -> Result<AgentSetup> {
    let mut prompts = AgentPrompts::default();
    let mut clients = HashMap::new();
    let mut settings = HashMap::new();

    // 必需的五个 agent
    for name in AGENTS {
        let ac = &cfg[*name];
        let prompt_path = ac
            .prompt
            .as_deref()
            .ok_or_else(|| anyhow!("agent '{name}' 缺少 prompt 配置"))?;
        let prompt_path = expand_tilde(prompt_path);
        let text = std::fs::read_to_string(&prompt_path).with_context(|| {
            format!(
                "读取 agent '{name}' 的提示词失败：{}（可用 `preparer prompt update {name} <文件>` 恢复）",
                prompt_path.display()
            )
        })?;
        anyhow::ensure!(
            !text.trim().is_empty(),
            "agent '{name}' 的提示词为空：{}",
            prompt_path.display()
        );
        prompts.set(
            crate::prompts::role_from_name(name).ok_or_else(|| anyhow!("未知 agent '{name}'"))?,
            text,
        );
        // base_url / api_key 任一设置即用 per-agent 客户端（缺省回退全局值）
        if ac.base_url.is_some() || ac.api_key.is_some() {
            let base = ac.base_url.clone().unwrap_or_else(|| global_base_url.to_string());
            let key = ac.api_key.clone().unwrap_or_else(|| global_api_key.to_string());
            let client = Client::new(base, key)?;
            clients.insert(name.to_string(), client);
        }
        settings.insert(
            name.to_string(),
            AgentSettings {
                reasoning: ac.reasoning,
                max_context: ac.max_context.unwrap_or(DEFAULT_MAX_CONTEXT),
                pricing: build_pricing(ac, global_base_url)?,
            },
        );
    }

    // 可选的 compactor：缺省回退 supervisor 客户端 + 内置提示词
    let mut compactor_prompt = DEFAULT_COMPACTOR_PROMPT.to_string();
    if let Some(ac) = cfg.get(COMPACTOR) {
        if let Some(p) = &ac.prompt {
            let path = expand_tilde(p);
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("读取 compactor 提示词失败：{}", path.display()))?;
            anyhow::ensure!(!text.trim().is_empty(), "compactor 的提示词为空：{}", path.display());
            compactor_prompt = text;
        }
        if ac.base_url.is_some() || ac.api_key.is_some() {
            let base = ac.base_url.clone().unwrap_or_else(|| global_base_url.to_string());
            let key = ac.api_key.clone().unwrap_or_else(|| global_api_key.to_string());
            clients.insert(COMPACTOR.to_string(), Client::new(base, key)?);
        }
        settings.insert(
            COMPACTOR.to_string(),
            AgentSettings {
                reasoning: ac.reasoning,
                max_context: ac.max_context.unwrap_or(DEFAULT_MAX_CONTEXT),
                pricing: build_pricing(ac, global_base_url)?,
            },
        );
    }

    Ok(AgentSetup { prompts, clients, settings, compactor_prompt })
}

/// 启动检查 + 加载。agents.json 不存在则报错。
pub fn require_agent_setup(global_base_url: &str, global_api_key: &str) -> Result<AgentSetup> {
    let cfg = load_agents_config()?;
    load_agent_setup(&cfg, global_base_url, global_api_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_works() {
        let _guard = crate::paths::tests::lock_home();
        #[allow(unused_unsafe)]
        unsafe { std::env::set_var("HOME", "/home/test"); }
        assert_eq!(expand_tilde("~/x.md"), PathBuf::from("/home/test/x.md"));
        assert_eq!(expand_tilde("/abs/x.md"), PathBuf::from("/abs/x.md"));
    }

    #[test]
    fn agent_config_new_fields_parse() {
        let cfg: AgentConfig = serde_json::from_str(
            r#"{
                "prompt": "p.md",
                "reasoning": true,
                "price": { "input": 3.0, "hit": 0.1, "output": 9.0, "currency": "￥" },
                "max_context": 65536
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.reasoning, Some(true));
        assert_eq!(cfg.max_context, Some(65536));
        let p = cfg.price.unwrap();
        assert_eq!(p.currency, "￥");
        assert_eq!(p.input, 3.0);

        let cfg2: AgentConfig =
            serde_json::from_str(r#"{ "prompt": "p.md", "price-policy": "auto" }"#).unwrap();
        assert_eq!(cfg2.price_policy.as_deref(), Some("auto"));
        assert_eq!(cfg2.reasoning, None);
    }

    #[test]
    fn build_pricing_fixed_overrides_policy() {
        let ac: AgentConfig = serde_json::from_str(
            r#"{ "prompt": "p", "price": { "input": 1, "hit": 0.1, "output": 2 }, "price-policy": "auto" }"#,
        )
        .unwrap();
        let p = build_pricing(&ac, "https://api.deepseek.com/v1").unwrap();
        assert!(matches!(p, Pricing::Fixed(_)));
    }

    #[test]
    fn build_pricing_auto_deepseek() {
        let ac: AgentConfig =
            serde_json::from_str(r#"{ "prompt": "p", "base_url": "https://api.deepseek.com/v1" }"#)
                .unwrap();
        let p = build_pricing(&ac, "https://x").unwrap();
        assert!(matches!(p, Pricing::DeepSeek));
    }

    #[test]
    fn build_pricing_rejects_unknown_policy() {
        let ac: AgentConfig =
            serde_json::from_str(r#"{ "prompt": "p", "price-policy": "magic" }"#).unwrap();
        assert!(build_pricing(&ac, "https://x").is_err());
    }
}
