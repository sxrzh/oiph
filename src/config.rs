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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::client::Client;
use crate::prompts::AgentPrompts;

pub const AGENTS: &[&str] = &["supervisor", "statement", "solution", "auxiliary", "searching"];

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
    pub prompt: String,
}

pub type AgentsConfig = HashMap<String, AgentConfig>;

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

/// 启动时加载的全部 agent 设置：提示词 + 每个 agent 的独立客户端。
pub struct AgentSetup {
    pub prompts: AgentPrompts,
    pub clients: HashMap<String, Client>,
}

/// 加载提示词文件并构建 per-agent 客户端。
pub fn load_agent_setup(
    cfg: &AgentsConfig,
    global_base_url: &str,
    global_api_key: &str,
) -> Result<AgentSetup> {
    let mut prompts = AgentPrompts::default();
    let mut clients = HashMap::new();
    for name in AGENTS {
        let ac = &cfg[*name];
        let prompt_path = expand_tilde(&ac.prompt);
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
    }
    Ok(AgentSetup { prompts, clients })
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
        #[allow(unused_unsafe)]
        unsafe { std::env::set_var("HOME", "/home/test"); }
        assert_eq!(expand_tilde("~/x.md"), PathBuf::from("/home/test/x.md"));
        assert_eq!(expand_tilde("/abs/x.md"), PathBuf::from("/abs/x.md"));
    }
}
