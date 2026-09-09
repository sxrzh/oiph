//! 固定路径：全局与工程两级的知识库 / skills / vendor 目录。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, bail};

/// `--user` 指定的家目录覆盖（仅 `oiph init --user` 进程内生效）。
static HOME_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// 注册家目录覆盖（init --user 用；重复注册报错）。
pub fn set_home_override(home: PathBuf) -> anyhow::Result<()> {
    HOME_OVERRIDE
        .set(home.clone())
        .map_err(|_| anyhow::anyhow!("家目录覆盖已被设置过：{}", home.display()))
}

/// 当前生效的家目录：覆盖值 > `home` crate（HOME 环境变量，缺省时回退
/// getpwuid 按当前用户查询）> 临时目录。
pub fn user_home() -> PathBuf {
    if let Some(h) = HOME_OVERRIDE.get() {
        return h.clone();
    }
    home::home_dir().unwrap_or_else(std::env::temp_dir)
}

/// 查询指定用户的家目录（getent passwd，缺 getent 时直接解析 /etc/passwd）。
pub fn lookup_user_home(name: &str) -> anyhow::Result<PathBuf> {
    let parse_line = |line: &str| -> Option<PathBuf> {
        line.trim()
            .split(':')
            .nth(5)
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
    };
    // NSS 感知（LDAP 等）；BusyBox/Alpine 也有 getent
    if let Ok(out) = std::process::Command::new("getent").arg("passwd").arg(name).output()
        && out.status.success()
        && let Some(home) = String::from_utf8_lossy(&out.stdout).lines().find_map(parse_line)
    {
        return Ok(home);
    }
    // 兜底：直接解析 /etc/passwd（macOS 本地用户也在其中）
    if let Ok(passwd) = std::fs::read_to_string("/etc/passwd")
        && let Some(home) = passwd
            .lines()
            .find(|l| l.split(':').next() == Some(name))
            .and_then(parse_line)
    {
        return Ok(home);
    }
    bail!("无法解析用户 {name} 的家目录（用户不存在？）")
}

/// 全局配置根目录：`$HOME/.oiph`。
pub fn oiph_home() -> PathBuf {
    user_home().join(".oiph")
}

/// 全局知识库目录。
pub fn global_kb_dir() -> PathBuf {
    oiph_home().join("kb")
}

/// 全局 skills 目录。
pub fn global_skills_dir() -> PathBuf {
    oiph_home().join("skills")
}

/// 全局 vendor 目录（第三方固定文件：testlib.h / testlib_lemon.h，init.sh 初始化）。
pub fn vendor_dir() -> PathBuf {
    oiph_home().join("vendor")
}

/// Web 前端目录：`~/.oiph/frontend/dist`（用户自行复制 frontend/dist 到此处）。
pub fn web_dist_dir() -> PathBuf {
    oiph_home().join("frontend").join("dist")
}

/// 读取 vendor 中的文件；不存在时报错（提示运行 init.sh）。
pub fn vendor_read(name: &str) -> anyhow::Result<String> {
    let p = vendor_dir().join(name);
    std::fs::read_to_string(&p).with_context(|| {
        format!(
            "读取 {} 失败，请先运行 init.sh 初始化（或检查 ~/.oiph/vendor/{name} 是否存在）",
            vendor_dir().display()
        )
    })
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
/// 测试辅助（HOME 环境变量沙盒；并行测试共享进程环境，需要串行化）。
#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};

    static HOME_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn lock_home() -> MutexGuard<'static, ()> {
        HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 沙盒 HOME：切到临时目录并写入 vendor 的两个 testlib。
    /// Guard 被 drop 时恢复原 HOME 并删除临时目录。
    pub(crate) fn sandbox_home_with_vendor(tag: &str) -> HomeGuard {
        let lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("HOME");
        let home = std::env::temp_dir()
            .join(format!("oiph_home_{tag}_{}", uuid::Uuid::new_v4()));
        let vendor = home.join(".oiph").join("vendor");
        std::fs::create_dir_all(&vendor).unwrap();
        std::fs::write(vendor.join("testlib.h"), "// vendor testlib (sandbox)\n").unwrap();
        std::fs::write(vendor.join("testlib_lemon.h"), "// vendor lemon testlib (sandbox)\n").unwrap();
        #[allow(unused_unsafe)]
        unsafe { std::env::set_var("HOME", &home); }
        HomeGuard { prev, home, _lock: lock }
    }

    pub(crate) struct HomeGuard {
        prev: Option<std::ffi::OsString>,
        home: PathBuf,
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            #[allow(unused_unsafe)]
            unsafe {
                match &self.prev {
                    Some(p) => std::env::set_var("HOME", p),
                    None => std::env::remove_var("HOME"),
                }
            }
            std::fs::remove_dir_all(&self.home).ok();
        }
    }

    #[test]
    fn paths_compose() {
        let p = Path::new("/tmp/proj");
        assert!(super::project_kb_dir(p).ends_with(".oiph/kb"));
        assert!(super::project_skills_dir(p).ends_with(".oiph/skills"));
        assert!(super::global_kb_dir().starts_with(super::oiph_home()));
    }

    #[test]
    fn lookup_user_home_root_and_unknown() {
        assert!(super::lookup_user_home("root").is_ok());
        let err = super::lookup_user_home("oiph_no_such_user_xyz").unwrap_err();
        assert!(err.to_string().contains("无法解析用户"));
    }

    #[test]
    fn user_home_follows_home_crate() {
        // 未设置 override 时与 home crate 一致（不做 HOME 环境变量变更，
        // 避免与其它沙盒测试竞态）
        let expected = home::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".oiph");
        assert_eq!(super::oiph_home(), expected);
    }
}
