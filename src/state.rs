//! 全局应用状态：客户端配置、当前比赛目录、知识库与 skills 目录。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::agent::AgentDeps;
use crate::client::Client;
use crate::dupcheck::Backend;
use crate::paths;
use crate::prompts::AgentPrompts;
use crate::tools::ToolContext;

pub struct App {
    pub root: PathBuf,
    current: Mutex<Option<PathBuf>>,
    pub base_url: String,
    pub api_key: String,
    pub embed_model: Option<String>,
    pub model: String,
    pub max_steps: usize,
    pub dup_backend: Backend,
    pub client: Client,
    /// 各 agent 的系统提示词（从 ~/.oiph/config 加载）。
    prompts: Mutex<AgentPrompts>,
    /// per-agent 客户端（agents.json 中配置了 base_url/api_key 的 agent）。
    agent_clients: Mutex<HashMap<String, Client>>,
    /// per-agent 运行设置（reasoning / max_context / pricing）。
    agent_settings: Mutex<HashMap<String, crate::config::AgentSettings>>,
    /// compactor 的压缩提示词（未配置时为内置默认）。
    compactor_prompt: Mutex<String>,
    /// 工作区快照：undo 栈（每步工具执行前的 tree hash + 对话消息数）。
    undo_stack: Mutex<Vec<crate::snapshot::SnapshotPoint>>,
    /// 工作区快照：redo 栈（undo 时保存的当前状态）。
    redo_stack: Mutex<Vec<crate::snapshot::SnapshotPoint>>,
    /// ask_user 问卷的答案回传通道（工具等待用户提交）。
    ask_answer: Mutex<Option<UnboundedSender<Value>>>,
}

impl App {
    pub fn new(
        root: PathBuf,
        base_url: String,
        api_key: String,
        embed_model: Option<String>,
        model: String,
        max_steps: usize,
        dup_backend: Backend,
    ) -> Result<Self> {
        let client = Client::new(base_url.clone(), api_key.clone())?;
        Ok(Self {
            root,
            current: Mutex::new(None),
            base_url,
            api_key,
            embed_model,
            model,
            max_steps,
            dup_backend,
            client,
            prompts: Mutex::new(AgentPrompts::default()),
            agent_clients: Mutex::new(HashMap::new()),
            agent_settings: Mutex::new(HashMap::new()),
            compactor_prompt: Mutex::new(crate::config::DEFAULT_COMPACTOR_PROMPT.to_string()),
            undo_stack: Mutex::new(Vec::new()),
            redo_stack: Mutex::new(Vec::new()),
            ask_answer: Mutex::new(None),
        })
    }

    /// 注册问卷答案接收端（ask_user 工具调用前），返回旧值。
    pub fn register_ask_answer(&self, tx: UnboundedSender<Value>) {
        if let Ok(mut g) = self.ask_answer.lock() {
            *g = Some(tx);
        }
    }

    /// 取下问卷答案接收端（工具结束时）。
    pub fn take_ask_answer(&self) {
        if let Ok(mut g) = self.ask_answer.lock() {
            *g = None;
        }
    }

    /// 前端提交问卷答案 / 取消。返回是否有接收端。
    pub fn send_ask_answer(&self, value: Value) -> bool {
        self.ask_answer
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|tx| tx.send(value).is_ok()))
            .unwrap_or(false)
    }

    /// 当前 session 的快照仓库（无比赛/无 session 时 None）。
    fn snapshot_store(&self) -> Option<crate::snapshot::SnapshotStore> {
        let cdir = self.contest_dir()?;
        let sess = crate::session::current_name(&cdir)?;
        Some(crate::snapshot::SnapshotStore::new(
            &crate::session::sessions_dir(&cdir).join(sess),
            &cdir,
        ))
    }

    /// 工具执行前捕获工作区快照（新操作清空 redo 栈）。
    /// 同时在对话区显示快照信息。
    pub fn snapshot_capture(&self, msg_len: usize) {
        let Some(store) = self.snapshot_store() else { return };
        if let Ok(h) = store.capture() {
            if let Ok(mut u) = self.undo_stack.lock() {
                u.push(crate::snapshot::SnapshotPoint { hash: h.clone(), msg_len });
            }
            if let Ok(mut r) = self.redo_stack.lock() {
                r.clear();
            }
            // 对话区显示（GUI 走 tool_result 消息；CLI 打印到终端）
            let short = &h[..8.min(h.len())];
            crate::term::send_tool_result(&format!("已建立快照 {short}"));
            crate::term::println_err(&format!("已建立快照 {short}"));
        }
    }

    /// 回滚到上一个快照（同时回退对话）。
    /// 返回应恢复到的快照点（调用方把消息截断到 `point.msg_len`）。
    pub fn snapshot_undo(&self, current_msg_len: usize) -> Result<Option<crate::snapshot::SnapshotPoint>> {
        let Some(store) = self.snapshot_store() else { return Ok(None) };
        let point = self.undo_stack.lock().ok().and_then(|mut u| u.pop());
        let Some(point) = point else { return Ok(None) };
        // 当前状态压入 redo
        if let Ok(cur) = store.capture()
            && let Ok(mut r) = self.redo_stack.lock() {
                r.push(crate::snapshot::SnapshotPoint { hash: cur, msg_len: current_msg_len });
            }
        store.restore(&point.hash)?;
        Ok(Some(point))
    }

    /// 重做：恢复 undo 前的状态（同时恢复对话）。
    /// 返回应恢复到的快照点。
    pub fn snapshot_redo(&self, current_msg_len: usize) -> Result<Option<crate::snapshot::SnapshotPoint>> {
        let Some(store) = self.snapshot_store() else { return Ok(None) };
        let point = self.redo_stack.lock().ok().and_then(|mut r| r.pop());
        let Some(point) = point else { return Ok(None) };
        if let Ok(cur) = store.capture()
            && let Ok(mut u) = self.undo_stack.lock() {
                u.push(crate::snapshot::SnapshotPoint { hash: cur, msg_len: current_msg_len });
            }
        store.restore(&point.hash)?;
        Ok(Some(point))
    }

    /// 设置 agent 提示词、per-agent 客户端与运行设置（启动时从配置加载）。
    pub fn set_agent_setup(
        &self,
        prompts: AgentPrompts,
        clients: HashMap<String, Client>,
        settings: HashMap<String, crate::config::AgentSettings>,
        compactor_prompt: String,
    ) {
        if let Ok(mut p) = self.prompts.lock() {
            *p = prompts;
        }
        if let Ok(mut c) = self.agent_clients.lock() {
            *c = clients;
        }
        if let Ok(mut s) = self.agent_settings.lock() {
            *s = settings;
        }
        if let Ok(mut p) = self.compactor_prompt.lock() {
            *p = compactor_prompt;
        }
    }

    /// agent 的系统提示词。
    pub fn prompt_for(&self, role_name: &str) -> String {
        self.prompts
            .lock()
            .ok()
            .and_then(|p| {
                crate::prompts::role_from_name(role_name)
                    .map(|r| p.get(r).to_string())
            })
            .unwrap_or_default()
    }

    /// compactor 的压缩提示词。
    pub fn compactor_prompt(&self) -> String {
        self.compactor_prompt
            .lock()
            .ok()
            .map(|p| p.clone())
            .unwrap_or_else(|| crate::config::DEFAULT_COMPACTOR_PROMPT.to_string())
    }

    /// agent 运行设置（reasoning / max_context / pricing）。
    /// compactor 未单独配置时回退 supervisor 的设置。
    pub fn settings_for(&self, name: &str) -> crate::config::AgentSettings {
        if let Ok(m) = self.agent_settings.lock() {
            if let Some(s) = m.get(name) {
                return s.clone();
            }
            if name == crate::config::COMPACTOR
                && let Some(s) = m.get("supervisor")
            {
                return s.clone();
            }
        }
        crate::config::AgentSettings::default()
    }

    /// 按 agent 名取客户端（未单独配置则回退全局客户端）。
    pub fn client_for(&self, role_name: &str) -> Option<Client> {
        self.agent_clients.lock().ok()?.get(role_name).cloned()
    }

    pub fn contest_dir(&self) -> Option<PathBuf> {
        self.current.lock().ok()?.clone()
    }

    pub fn set_contest_dir(&self, p: Option<PathBuf>) {
        if let Ok(mut g) = self.current.lock() {
            *g = p;
        }
    }

    /// 工程目录：当前比赛目录；没有比赛时退回工作目录。
    pub fn project_dir(&self) -> PathBuf {
        self.contest_dir().unwrap_or_else(|| self.root.clone())
    }

    /// 知识库目录：全局 + 工程。
    pub fn kb_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![paths::global_kb_dir()];
        dirs.push(paths::project_kb_dir(&self.project_dir()));
        dirs
    }

    /// skills 根目录：全局 + 工程（工程在后，同名时工程覆盖全局）。
    pub fn skill_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![paths::global_skills_dir()];
        roots.push(paths::project_skills_dir(&self.project_dir()));
        roots
    }

    pub fn tool_ctx(&self, workdir: &Path) -> ToolContext {
        ToolContext {
            workdir: workdir.to_path_buf(),
            kb_dirs: self.kb_dirs(),
            skill_roots: self.skill_roots(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            embed_model: self.embed_model.clone(),
            dup_backend: self.dup_backend,
        }
    }

    pub fn deps(&self) -> AgentDeps<'_> {
        AgentDeps {
            model: &self.model,
            max_steps: self.max_steps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_contest_dir_roundtrip() {
        let app = App::new(
            PathBuf::from("/tmp"),
            "http://localhost:1/v1".into(),
            "k".into(),
            None,
            "m".into(),
            10,
            Backend::Cpret,
        )
        .unwrap();
        assert!(app.contest_dir().is_none());
        app.set_contest_dir(Some(PathBuf::from("/tmp/c")));
        assert_eq!(app.contest_dir(), Some(PathBuf::from("/tmp/c")));
        app.set_contest_dir(None);
        assert!(app.contest_dir().is_none());
    }

    #[test]
    fn dirs_follow_contest() {
        let app = App::new(
            PathBuf::from("/tmp"),
            "http://localhost:1/v1".into(),
            "k".into(),
            None,
            "m".into(),
            10,
            Backend::Cpret,
        )
        .unwrap();
        let d = app.kb_dirs();
        assert_eq!(d.len(), 2);
        assert!(d[1].ends_with(".oiph/kb"));
        let r = app.skill_roots();
        assert_eq!(r.len(), 2);
        assert!(r[1].ends_with(".oiph/skills"));
    }
}
