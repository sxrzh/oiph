//! 固定路径：全局与工程两级的知识库 / skills 目录。

use std::path::{Path, PathBuf};

/// 全局配置根目录：`$HOME/.oiph`。
pub fn oiph_home() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".oiph")
}

/// 全局知识库目录。
pub fn global_kb_dir() -> PathBuf {
    oiph_home().join("kb")
}

/// 全局 skills 目录。
pub fn global_skills_dir() -> PathBuf {
    oiph_home().join("skills")
}

/// 工程（比赛）目录下的配置目录名。
pub const PROJECT_DOTDIR: &str = ".oiph";

/// 工程知识库目录：`<工程>/.oiph/kb`。
pub fn project_kb_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(PROJECT_DOTDIR).join("kb")
}

/// 工程 skills 目录：`<工程>/.oiph/skills`。
pub fn project_skills_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(PROJECT_DOTDIR).join("skills")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_compose() {
        let p = Path::new("/tmp/proj");
        assert!(project_kb_dir(p).ends_with(".oiph/kb"));
        assert!(project_skills_dir(p).ends_with(".oiph/skills"));
        assert!(global_kb_dir().starts_with(oiph_home()));
    }
}
