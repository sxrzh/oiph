//! 工作区快照：每个主 session 一个独立 git 仓库（`<session>/snapshot/`）。
//!
//! 参考 opencode 的工作方式：
//! - 捕获：`git add -A` + `git write-tree` 记录工作区状态为 tree hash（不产生 commit）
//! - 恢复：`git read-tree <hash>` + `git checkout-index -a -f` 物理还原文件，
//!   再删除工作区中未被该 tree 跟踪的文件（还原被修改文件、删除新建文件）
//!
//! 快照只覆盖比赛工程目录（workdir），不影响工程本身的 git 仓库：
//! snapshot 仓库通过 `--git-dir`/`--work-tree` 指向独立目录。

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

/// 快照点：tree hash + 捕获时的对话消息数（undo/redo 同时回退对话）。
#[derive(Debug, Clone)]
pub struct SnapshotPoint {
    pub hash: String,
    pub msg_len: usize,
}

pub struct SnapshotStore {
    /// snapshot git 目录：<session>/snapshot/.git（独立仓库）
    git_dir: PathBuf,
    /// 被跟踪的工作区：比赛工程目录
    work_tree: PathBuf,
}

impl SnapshotStore {
    pub fn new(session_dir: &Path, work_tree: &Path) -> Self {
        Self {
            git_dir: session_dir.join("snapshot").join(".git"),
            work_tree: work_tree.to_path_buf(),
        }
    }

    /// 临时 index 文件路径（所有操作共用，与真实 git index 隔离）。
    fn index_file(&self) -> PathBuf {
        self.git_dir.parent().unwrap().join("index.tmp")
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        // 防御：清掉上次异常退出遗留的 index 锁（锁与 index 同目录，名字加 .lock 后缀）
        let mut lock = self.index_file().into_os_string();
        lock.push(".lock");
        let _ = std::fs::remove_file(PathBuf::from(lock));
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("--work-tree")
            .arg(&self.work_tree)
            // 所有操作共用隔离的 index（write-tree/read-tree 等都依赖它）
            .env("GIT_INDEX_FILE", self.index_file())
            .args(args)
            .output()
            .map_err(|e| anyhow!("git 启动失败：{e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("git {} 失败：{}", args.join(" "), stderr.trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// 确保快照仓库已初始化。
    pub fn ensure_init(&self) -> Result<()> {
        if self.git_dir.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.git_dir)
            .with_context(|| format!("创建 {} 失败", self.git_dir.display()))?;
        // bare init 不接受 --work-tree，单独调用
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .args(["init", "--bare", "--quiet"])
            .output()
            .map_err(|e| anyhow!("git 启动失败：{e}"))?;
        if !out.status.success() {
            anyhow::bail!(
                "git init 失败：{}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    /// 捕获当前工作区状态，返回 tree hash。
    /// 使用隔离的临时 index，不污染工作区的 .git。排除 `.oiph/`（session churn）。
    pub fn capture(&self) -> Result<String> {
        self.ensure_init()?;
        // 清空临时 index（git add -A 基于其当前内容工作）
        let _ = std::fs::remove_file(self.index_file());
        self.git(&["add", "-A", "--force", "--", ".", ":(exclude).oiph"])?;
        let hash = self.git(&["write-tree"])?;
        Ok(hash)
    }

    /// 恢复工作区到指定 tree hash。
    pub fn restore(&self, tree: &str) -> Result<()> {
        self.ensure_init()?;
        // read-tree 把 tree 读入 index
        self.git(&["read-tree", tree])?;
        // checkout-index 物理写回文件（覆盖被修改的）
        self.git(&["checkout-index", "-a", "-f", "--quiet"])?;
        // 删除工作区中不属于该 tree 的文件（新建的文件）
        self.remove_untracked()?;
        Ok(())
    }

    /// 删除 work_tree 中存在但 index/tree 中没有的文件。
    fn remove_untracked(&self) -> Result<()> {
        // ls-files 列出当前 index（即 tree）中的所有文件
        let out = self.git(&["ls-files", "-z"])?;
        let tracked: std::collections::HashSet<PathBuf> = out
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();

        // 遍历 work_tree，删除不在 tree 中的文件
        // （跳过 .oiph 整棵树——session/snapshot 目录不归快照管）
        remove_untracked_files(&self.work_tree, &self.work_tree, &tracked)?;
        Ok(())
    }
}

fn remove_untracked_files(
    root: &Path,
    dir: &Path,
    tracked: &std::collections::HashSet<PathBuf>,
) -> Result<()> {
    // 跳过 .oiph（会话/快照目录）和所有 .git 目录
    if dir.file_name().is_some_and(|n| n == ".oiph" || n == ".git") {
        return Ok(());
    }
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() {
            remove_untracked_files(root, &p, tracked)?;
            // 若目录为空则删除
            let _ = std::fs::remove_dir(&p);
        } else {
            let rel = p.strip_prefix(root)?.to_path_buf();
            if !tracked.contains(&rel) {
                std::fs::remove_file(&p)
                    .with_context(|| format!("删除未跟踪文件 {} 失败", p.display()))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_restore_roundtrip() {
        let root = std::env::temp_dir().join(format!("prep_snap_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let sess = root.join("sess");
        std::fs::create_dir_all(&sess).unwrap();

        // 工作区初始状态：一个文件
        std::fs::write(root.join("a.txt"), "v1").unwrap();

        let store = SnapshotStore::new(&sess, &root);
        let h1 = store.capture().unwrap();
        assert!(!h1.is_empty());

        // 修改 + 新建文件
        std::fs::write(root.join("a.txt"), "v2").unwrap();
        std::fs::write(root.join("b.txt"), "new").unwrap();
        let h2 = store.capture().unwrap();
        assert_ne!(h1, h2);

        // 恢复到 h1：a.txt 回 v1，b.txt 被删除
        store.restore(&h1).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "v1");
        assert!(!root.join("b.txt").exists());

        // 恢复到 h2
        store.restore(&h2).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "v2");
        assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "new");

        std::fs::remove_dir_all(&root).ok();
    }
}
