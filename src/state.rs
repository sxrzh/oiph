//! 全局应用状态：客户端配置、当前比赛目录、知识库与 skills 目录。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;

use crate::agent::AgentDeps;
use crate::client::Client;
use crate::dupcheck::Backend;
use crate::paths;
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
        })
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
            client: &self.client,
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
