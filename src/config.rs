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
use std::path::PathBuf;

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

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    /// 模型名称（必需；未设置时该 agent 调用会报错提示配置）。
    #[serde(default)]
    pub model: Option<String>,
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
    /// 模型名称。
    pub model: Option<String>,
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
            model: None,
            reasoning: None,
            max_context: DEFAULT_MAX_CONTEXT,
            pricing: Pricing::None,
        }
    }
}

/// 展开 `~` 前缀。
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return crate::paths::user_home().join(rest);
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

/// 保存 agents.json（设置界面用）。
pub fn save_agents_config(cfg: &AgentsConfig) -> Result<()> {
    let path = agents_config_path();
    std::fs::create_dir_all(config_dir())?;
    let json = serde_json::to_vec_pretty(cfg)?;
    std::fs::write(&path, json).with_context(|| format!("写入 {}", path.display()))?;
    Ok(())
}

/// 启动时加载的全部 agent 设置：提示词 + 每个 agent 的独立客户端 + 运行设置。
pub struct AgentSetup {
    pub prompts: AgentPrompts,
    pub clients: HashMap<String, Client>,
    pub settings: HashMap<String, AgentSettings>,
    /// compactor 的压缩提示词（未配置时为内置默认）。
    pub compactor_prompt: String,
}

/// 读取非空环境变量。
fn env_non_empty(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|s| !s.trim().is_empty())
}

/// 解析单个 agent 的有效配置：自身显式值 → 环境变量
/// （OPENAI_BASE_URL / OPENAI_API_KEY / OPENAI_MODEL）。
fn resolve_agent(ac: &AgentConfig) -> AgentConfig {
    let mut resolved = ac.clone();
    resolved.base_url = ac
        .base_url
        .clone()
        .or_else(|| env_non_empty("OPENAI_BASE_URL"));
    resolved.api_key = ac
        .api_key
        .clone()
        .or_else(|| env_non_empty("OPENAI_API_KEY"));
    resolved.model = ac.model.clone().or_else(|| env_non_empty("OPENAI_MODEL"));
    resolved
}

/// 构建单个 agent 的计价策略。
fn build_pricing(ac: &AgentConfig) -> Result<Pricing> {
    if let Some(p) = &ac.price {
        return Ok(Pricing::fixed(p.clone()));
    }
    if let Some(policy) = &ac.price_policy
        && policy != "auto" {
            bail!("不支持的 price-policy：'{policy}'（目前仅支持 \"auto\"）");
        }
    // price 与 price-policy 都没有 → auto 模式（base_url 为空时 pricing::auto 回退环境变量）
    Ok(crate::pricing::auto(ac.base_url.as_deref().unwrap_or("")))
}

/// 加载提示词文件并构建 per-agent 客户端与运行设置。
/// 每个 agent 的 base_url/api_key/model 按"自身显式值 → 环境变量"回退解析；
/// 解析后同时配置了 base_url 与 api_key 才构建独立客户端。
pub fn load_agent_setup(cfg: &AgentsConfig) -> Result<AgentSetup> {
    let mut prompts = AgentPrompts::default();
    let mut clients = HashMap::new();
    let mut settings = HashMap::new();

    // 必需的五个 agent
    for name in AGENTS {
        let ac = &resolve_agent(&cfg[*name]);
        let prompt_path = ac
            .prompt
            .as_deref()
            .ok_or_else(|| anyhow!("agent '{name}' 缺少 prompt 配置"))?;
        let prompt_path = expand_tilde(prompt_path);
        let text = std::fs::read_to_string(&prompt_path).with_context(|| {
            format!(
                "读取 agent '{name}' 的提示词失败：{}（可用 `oiph prompt update {name} <文件>` 恢复）",
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
        // base_url 与 api_key 都配置时才用独立客户端（否则运行时回退 supervisor）
        if let (Some(base), Some(key)) = (&ac.base_url, &ac.api_key) {
            let client = Client::new(base.clone(), key.clone())?;
            clients.insert(name.to_string(), client);
        }
        settings.insert(
            name.to_string(),
            AgentSettings {
                model: ac.model.clone(),
                reasoning: ac.reasoning,
                max_context: ac.max_context.unwrap_or(DEFAULT_MAX_CONTEXT),
                pricing: build_pricing(ac)?,
            },
        );
    }

    // 可选的 compactor：缺省回退 supervisor 客户端 + 内置提示词
    let mut compactor_prompt = DEFAULT_COMPACTOR_PROMPT.to_string();
    if let Some(raw) = cfg.get(COMPACTOR) {
        let ac = &resolve_agent(raw);
        if let Some(p) = &ac.prompt {
            let path = expand_tilde(p);
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("读取 compactor 提示词失败：{}", path.display()))?;
            anyhow::ensure!(!text.trim().is_empty(), "compactor 的提示词为空：{}", path.display());
            compactor_prompt = text;
        }
        if let (Some(base), Some(key)) = (&ac.base_url, &ac.api_key) {
            clients.insert(COMPACTOR.to_string(), Client::new(base.clone(), key.clone())?);
        }
        settings.insert(
            COMPACTOR.to_string(),
            AgentSettings {
                model: ac.model.clone(),
                reasoning: ac.reasoning,
                max_context: ac.max_context.unwrap_or(DEFAULT_MAX_CONTEXT),
                pricing: build_pricing(ac)?,
            },
        );
    }

    Ok(AgentSetup { prompts, clients, settings, compactor_prompt })
}

/// 启动检查 + 加载。agents.json 不存在则报错。
pub fn require_agent_setup() -> Result<AgentSetup> {
    let cfg = load_agents_config()?;
    load_agent_setup(&cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_works() {
        let guard = crate::paths::tests::sandbox_home("tilde");
        assert_eq!(expand_tilde("~/x.md"), guard.home().join("x.md"));
        assert_eq!(expand_tilde("/abs/x.md"), PathBuf::from("/abs/x.md"));
    }

    #[test]
    fn agent_config_new_fields_parse() {
        let cfg: AgentConfig = serde_json::from_str(
            r#"{
                "prompt": "p.md",
                "model": "deepseek-v4-flash",
                "reasoning": true,
                "price": { "input": 3.0, "hit": 0.1, "output": 9.0, "currency": "￥" },
                "max_context": 65536
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.model.as_deref(), Some("deepseek-v4-flash"));
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
        let p = build_pricing(&ac).unwrap();
        assert!(matches!(p, Pricing::Fixed(_)));
    }

    #[test]
    fn build_pricing_auto_deepseek() {
        let ac: AgentConfig =
            serde_json::from_str(r#"{ "prompt": "p", "base_url": "https://api.deepseek.com/v1" }"#)
                .unwrap();
        let p = build_pricing(&ac).unwrap();
        assert!(matches!(p, Pricing::DeepSeek));
    }

    #[test]
    fn build_pricing_rejects_unknown_policy() {
        let ac: AgentConfig =
            serde_json::from_str(r#"{ "prompt": "p", "price-policy": "magic" }"#).unwrap();
        assert!(build_pricing(&ac).is_err());
    }

    #[test]
    fn resolve_agent_env_fallback() {
        let _guard = crate::paths::tests::lock_home();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("OPENAI_BASE_URL", "https://api.deepseek.com/v1");
            std::env::set_var("OPENAI_API_KEY", "sk-test");
            std::env::set_var("OPENAI_MODEL", "deepseek-v4-flash");
        }
        // 全 null 的 agent：直接回退环境变量
        let empty: AgentConfig = serde_json::from_str(r#"{ "prompt": "p" }"#).unwrap();
        let r = resolve_agent(&empty);
        assert_eq!(r.base_url.as_deref(), Some("https://api.deepseek.com/v1"));
        assert_eq!(r.api_key.as_deref(), Some("sk-test"));
        assert_eq!(r.model.as_deref(), Some("deepseek-v4-flash"));
        // 自身显式值最优先
        let own: AgentConfig =
            serde_json::from_str(r#"{ "prompt": "p", "model": "own-model" }"#).unwrap();
        let r2 = resolve_agent(&own);
        assert_eq!(r2.model.as_deref(), Some("own-model"));

        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var("OPENAI_BASE_URL");
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("OPENAI_MODEL");
        }
    }
}
