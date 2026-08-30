//! 会话持久化：把 supervisor 对话保存为 JSON，存于 `<比赛工程>/.oiph/sessions/`。
//!
//! - 启动时默认加载上一次的 session（按 `current` 指针，缺失则取最近修改的）。
//! - 支持新建 / 切换 / 删除 / 导出 session。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::client::Message;

pub const SESSIONS_DIR_NAME: &str = "sessions";
pub const CURRENT_FILE: &str = "current";
pub const NAME_PREFIX: &str = "session-";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub name: String,
    pub updated_at: DateTime<Utc>,
    pub messages: usize,
    pub current: bool,
}

pub fn sessions_dir(contest_dir: &Path) -> PathBuf {
    contest_dir.join(".oiph").join(SESSIONS_DIR_NAME)
}

fn session_path(contest_dir: &Path, name: &str) -> PathBuf {
    sessions_dir(contest_dir).join(format!("{name}.json"))
}

fn current_path(contest_dir: &Path) -> PathBuf {
    sessions_dir(contest_dir).join(CURRENT_FILE)
}

/// 校验 session 名称：不能含路径分隔符、空白、'..'，也不能是保留名 `current`。
pub fn sanitize_name(name: &str) -> Result<String> {
    if name.is_empty() {
        bail!("session 名称不能为空");
    }
    if name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains(char::is_whitespace)
    {
        bail!("session 名称不能包含路径分隔符、'..' 或空白字符");
    }
    if name.eq_ignore_ascii_case(CURRENT_FILE) || name.ends_with(".json") {
        bail!("session 名称不能使用保留名或以 .json 结尾");
    }
    Ok(name.to_string())
}

/// 生成不冲突的自动名称：`session-YYYYMMDD-HHMMSS`，冲突则追加 -2/-3。
pub fn auto_name(contest_dir: &Path) -> Result<String> {
    let base = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let candidate = format!("{NAME_PREFIX}{base}");
    if !session_path(contest_dir, &candidate).exists() {
        return Ok(candidate);
    }
    for i in 2..1000 {
        let c = format!("{NAME_PREFIX}{base}-{i}");
        if !session_path(contest_dir, &c).exists() {
            return Ok(c);
        }
    }
    bail!("无法生成不冲突的 session 名称");
}

pub fn exists(contest_dir: &Path, name: &str) -> bool {
    session_path(contest_dir, name).is_file()
}

pub fn current_name(contest_dir: &Path) -> Option<String> {
    let p = current_path(contest_dir);
    std::fs::read_to_string(&p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|n| !n.is_empty() && exists(contest_dir, n))
}

pub fn set_current(contest_dir: &Path, name: &str) -> Result<()> {
    let dir = sessions_dir(contest_dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("创建 {} 失败", dir.display()))?;
    std::fs::write(current_path(contest_dir), name)
        .with_context(|| "写入 current 指针失败")?;
    Ok(())
}

pub fn clear_current(contest_dir: &Path) {
    let p = current_path(contest_dir);
    if p.exists() {
        let _ = std::fs::remove_file(p);
    }
}

pub fn load(contest_dir: &Path, name: &str) -> Result<Session> {
    let p = session_path(contest_dir, name);
    let raw = std::fs::read_to_string(&p)
        .with_context(|| format!("读取 session '{name}' 失败：{}", p.display()))?;
    serde_json::from_str::<Session>(&raw)
        .with_context(|| format!("解析 session '{name}' 失败"))
}

pub fn save(contest_dir: &Path, session: &Session) -> Result<()> {
    let dir = sessions_dir(contest_dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("创建 {} 失败", dir.display()))?;
    let json = serde_json::to_vec_pretty(session).context("序列化 session 失败")?;
    std::fs::write(session_path(contest_dir, &session.name), json)
        .with_context(|| "写入 session 失败")?;
    set_current(contest_dir, &session.name)?;
    Ok(())
}

/// 保存消息到指定 session（保留原 created_at，更新 updated_at）。
/// 若 session 文件不存在则新建。
pub fn save_messages(contest_dir: &Path, name: &str, messages: &[Message]) -> Result<()> {
    let now = Utc::now();
    let created_at = load(contest_dir, name)
        .ok()
        .map(|s| s.created_at)
        .unwrap_or(now);
    let session = Session {
        name: name.to_string(),
        created_at,
        updated_at: now,
        messages: messages.to_vec(),
    };
    save(contest_dir, &session)
}

/// 删除 session 文件（不删 current 指针，由调用方处理）。
pub fn delete(contest_dir: &Path, name: &str) -> Result<()> {
    let p = session_path(contest_dir, name);
    if !p.exists() {
        bail!("session '{name}' 不存在");
    }
    std::fs::remove_file(&p).with_context(|| format!("删除 session '{name}' 失败"))?;
    Ok(())
}

/// 列出所有 session 元信息，按 updated_at 降序。
pub fn list(contest_dir: &Path) -> Result<Vec<SessionMeta>> {
    let dir = sessions_dir(contest_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let cur = current_name(contest_dir);
    let mut metas = Vec::new();
    for e in std::fs::read_dir(&dir)
        .with_context(|| format!("读取 {} 失败", dir.display()))?
        .flatten()
    {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let fname = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Some(name) = fname.strip_suffix(".json") else {
            continue;
        };
        if name == CURRENT_FILE {
            continue;
        }
        match std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str::<Session>(&s).ok())
        {
            Some(s) => metas.push(SessionMeta {
                name: s.name.clone(),
                updated_at: s.updated_at,
                messages: s.messages.len(),
                current: cur.as_deref() == Some(s.name.as_str()),
            }),
            None => {
                // 损坏的 session 文件，按文件名与修改时间兜底
                if let Ok(meta) = e.metadata()
                    && let Ok(mtime) = meta.modified() {
                        let dt: DateTime<Utc> = mtime.into();
                        metas.push(SessionMeta {
                            name: name.to_string(),
                            updated_at: dt,
                            messages: 0,
                            current: cur.as_deref() == Some(name),
                        });
                    }
            }
        }
    }
    metas.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
    Ok(metas)
}

/// 加载上一次的 session：优先 current 指针，缺失则取最近修改的。没有则返回 None。
pub fn last(contest_dir: &Path) -> Result<Option<Session>> {
    if let Some(name) = current_name(contest_dir)
        && let Ok(s) = load(contest_dir, &name) {
            return Ok(Some(s));
        }
    let metas = list(contest_dir)?;
    if let Some(m) = metas.first() {
        set_current(contest_dir, &m.name)?;
        return load(contest_dir, &m.name).map(Some);
    }
    Ok(None)
}

/// 导出为 markdown 文本。
pub fn export_markdown(s: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Session: {}\n\n创建：{}\n更新：{}\n消息数：{}\n\n",
        s.name,
        s.created_at.format("%Y-%m-%d %H:%M:%S"),
        s.updated_at.format("%Y-%m-%d %H:%M:%S"),
        s.messages.len()
    ));
    for m in &s.messages {
        out.push_str(&format!("## {}\n\n", m.role));
        if let Some(c) = &m.content
            && !c.is_empty() {
                out.push_str(c);
                out.push_str("\n\n");
            }
        if let Some(tcs) = &m.tool_calls {
            for tc in tcs {
                out.push_str(&format!(
                    "> tool_call: `{}` `{}`\n",
                    tc.function.name, tc.function.arguments
                ));
            }
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Message;

    fn tmp_contest() -> PathBuf {
        let d = std::env::temp_dir().join(format!("prep_sess_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_load_list_roundtrip() {
        let c = tmp_contest();
        let msgs = vec![Message::system("sys"), Message::user("hi")];
        save_messages(&c, "s1", &msgs).unwrap();
        assert_eq!(current_name(&c).as_deref(), Some("s1"));

        let s = load(&c, "s1").unwrap();
        assert_eq!(s.name, "s1");
        assert_eq!(s.messages.len(), 2);

        let metas = list(&c).unwrap();
        assert_eq!(metas.len(), 1);
        assert!(metas[0].current);
        assert_eq!(metas[0].messages, 2);

        std::fs::remove_dir_all(&c).ok();
    }

    #[test]
    fn last_uses_current_pointer() {
        let c = tmp_contest();
        save_messages(&c, "a", &[Message::user("a")]).unwrap();
        // 制造时间差
        std::thread::sleep(std::time::Duration::from_millis(1100));
        save_messages(&c, "b", &[Message::user("b")]).unwrap();
        // current 指向 a（较旧），last 应返回 a
        set_current(&c, "a").unwrap();
        let s = last(&c).unwrap().unwrap();
        assert_eq!(s.name, "a");
        // 清指针后应取最近修改的 b
        clear_current(&c);
        let s = last(&c).unwrap().unwrap();
        assert_eq!(s.name, "b");
        std::fs::remove_dir_all(&c).ok();
    }

    #[test]
    fn auto_name_unique() {
        let c = tmp_contest();
        let n1 = auto_name(&c).unwrap();
        save_messages(&c, &n1, &[Message::user("x")]).unwrap();
        // 同秒内再生成应追加后缀
        let n2 = auto_name(&c).unwrap();
        assert_ne!(n1, n2);
        std::fs::remove_dir_all(&c).ok();
    }

    #[test]
    fn sanitize_rejects_bad() {
        assert!(sanitize_name("a b").is_err());
        assert!(sanitize_name("../x").is_err());
        assert!(sanitize_name("current").is_err());
        assert!(sanitize_name("ok-1").is_ok());
    }

    #[test]
    fn export_markdown_contains_messages() {
        let s = Session {
            name: "t".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            messages: vec![Message::user("hello")],
        };
        let md = export_markdown(&s);
        assert!(md.contains("hello"));
        assert!(md.contains("## user"));
    }
}
