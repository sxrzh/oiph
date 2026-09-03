//! 会话持久化：主 session 为目录，含 main.json + 子 agent 的 sub-N.json。
//!
//! 目录结构：
//! ```text
//! .oiph/sessions/
//!   current                 # 指向当前 session 名
//!   session-XXX/            # session 目录
//!     main.json             # 主 session（supervisor 消息 + children 引用）
//!     sub-1.json            # 子 agent session
//!     sub-2.json
//! ```


use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::client::Message;

pub const SESSIONS_DIR_NAME: &str = "sessions";
pub const CURRENT_FILE: &str = "current";
pub const NAME_PREFIX: &str = "session-";
pub const MAIN_FILE: &str = "main.json";

/// 子 session 引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildRef {
    pub filename: String, // e.g. "sub-1.json"
    pub agent: String,    // e.g. "solution"
    pub summary: String,  // 简要描述
}

/// 主 session。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub children: Vec<ChildRef>,
}

/// 子 session（仅消息列表）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubSession {
    pub agent: String,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub name: String,
    pub updated_at: DateTime<Utc>,
    pub messages: usize,
    pub current: bool,
}

// 待保存的子 agent session（全局而非 thread_local：tokio 任务可能迁移 worker 线程）
static PENDING_SUB_SESSIONS: std::sync::Mutex<Vec<(String, Vec<Message>, String)>> =
    std::sync::Mutex::new(Vec::new());

/// 记录一个子 agent 会话，待主 session 保存时一并写入文件。
pub fn push_pending_sub_session(agent: String, messages: Vec<Message>, summary: String) {
    PENDING_SUB_SESSIONS.lock().unwrap().push((agent, messages, summary));
}

fn take_pending_sub_sessions() -> Vec<(String, Vec<Message>, String)> {
    std::mem::take(&mut *PENDING_SUB_SESSIONS.lock().unwrap())
}

pub fn sessions_dir(contest_dir: &Path) -> PathBuf {
    contest_dir.join(".oiph").join(SESSIONS_DIR_NAME)
}

/// session 目录路径
fn session_dir(contest_dir: &Path, name: &str) -> PathBuf {
    sessions_dir(contest_dir).join(name)
}

/// 主 session 文件路径
fn main_path(contest_dir: &Path, name: &str) -> PathBuf {
    session_dir(contest_dir, name).join(MAIN_FILE)
}

fn current_path(contest_dir: &Path) -> PathBuf {
    sessions_dir(contest_dir).join(CURRENT_FILE)
}

pub fn sanitize_name(name: &str) -> Result<String> {
    if name.is_empty() {
        bail!("session 名称不能为空");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.contains(char::is_whitespace) {
        bail!("session 名称不能包含路径分隔符、'..' 或空白字符");
    }
    if name.eq_ignore_ascii_case(CURRENT_FILE) {
        bail!("session 名称不能使用保留名 current");
    }
    Ok(name.to_string())
}

pub fn auto_name(contest_dir: &Path) -> Result<String> {
    let base = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let candidate = format!("{NAME_PREFIX}{base}");
    if !session_dir(contest_dir, &candidate).exists() {
        return Ok(candidate);
    }
    for i in 2..1000 {
        let c = format!("{NAME_PREFIX}{base}-{i}");
        if !session_dir(contest_dir, &c).exists() {
            return Ok(c);
        }
    }
    bail!("无法生成不冲突的 session 名称");
}

pub fn exists(contest_dir: &Path, name: &str) -> bool {
    main_path(contest_dir, name).is_file()
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
    std::fs::create_dir_all(&dir)?;
    std::fs::write(current_path(contest_dir), name)?;
    Ok(())
}

pub fn clear_current(contest_dir: &Path) {
    let p = current_path(contest_dir);
    if p.exists() {
        let _ = std::fs::remove_file(p);
    }
}

pub fn load(contest_dir: &Path, name: &str) -> Result<Session> {
    let p = main_path(contest_dir, name);
    let raw = std::fs::read_to_string(&p)
        .with_context(|| format!("读取 session '{name}' 失败：{}", p.display()))?;
    let mut session: Session = serde_json::from_str(&raw)
        .with_context(|| format!("解析 session '{name}' 失败"))?;
    repair_tool_history(&mut session.messages);
    Ok(session)
}

/// 修复不完整的 tool_calls 历史：assistant 消息中的每个 tool_call_id
/// 必须有对应 tool 消息，否则在下一次 API 调用时被拒绝（HTTP 400）。
/// 旧版本中止逻辑可能产生缺失，此处自动补占位 tool 消息。
pub fn repair_tool_history(messages: &mut Vec<Message>) {
    // 收集已有 tool 结果的 id
    let answered: std::collections::HashSet<String> = messages
        .iter()
        .filter(|m| m.role == "tool")
        .filter_map(|m| m.tool_call_id.clone())
        .collect();
    // 找出缺失的 tool_call_id，在其 assistant 消息后插入占位
    let mut fixes: Vec<(usize, String)> = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        if m.role == "assistant"
            && let Some(tcs) = &m.tool_calls
        {
            for tc in tcs {
                if !answered.contains(&tc.id) {
                    fixes.push((i, tc.id.clone()));
                }
            }
        }
    }
    // 从后往前插入，避免索引失效
    for (i, id) in fixes.into_iter().rev() {
        messages.insert(
            i + 1,
            Message::tool("[工具被用户中止]".into(), id),
        );
    }
}

/// 加载子 session
pub fn load_sub(contest_dir: &Path, session_name: &str, filename: &str) -> Result<SubSession> {
    let p = session_dir(contest_dir, session_name).join(filename);
    let raw = std::fs::read_to_string(&p)
        .with_context(|| format!("读取子 session '{filename}' 失败"))?;
    serde_json::from_str::<SubSession>(&raw)
        .with_context(|| format!("解析子 session '{filename}' 失败"))
}

pub fn save(contest_dir: &Path, session: &Session) -> Result<()> {
    let dir = session_dir(contest_dir, &session.name);
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(session)?;
    std::fs::write(main_path(contest_dir, &session.name), json)?;
    set_current(contest_dir, &session.name)?;
    Ok(())
}

/// 取出 pending 子 session 并保存为文件，返回 ChildRef 列表。
/// 保留已有 children，新 children 追加在后。
pub fn flush_pending_sub_sessions(
    contest_dir: &Path,
    session_name: &str,
    existing_children: &[ChildRef],
) -> Result<Vec<ChildRef>> {
    let pending = take_pending_sub_sessions();
    if pending.is_empty() {
        return Ok(existing_children.to_vec());
    }
    let dir = session_dir(contest_dir, session_name);
    std::fs::create_dir_all(&dir)?;
    let mut children = existing_children.to_vec();
    let start = children.len() + 1;
    for (i, (agent, messages, summary)) in pending.into_iter().enumerate() {
        let filename = format!("sub-{}.json", i + start);
        let sub = SubSession { agent: agent.clone(), messages };
        let json = serde_json::to_vec_pretty(&sub)?;
        std::fs::write(dir.join(&filename), json)?;
        children.push(ChildRef { filename, agent, summary });
    }
    Ok(children)
}

/// 保存消息到指定 session（含子 session）。
pub fn save_messages(contest_dir: &Path, name: &str, messages: &[Message]) -> Result<()> {
    let now = Utc::now();
    let (created_at, existing_children) = load(contest_dir, name)
        .map(|s| (s.created_at, s.children))
        .unwrap_or((now, Vec::new()));
    let children = flush_pending_sub_sessions(contest_dir, name, &existing_children)?;
    let session = Session {
        name: name.to_string(),
        created_at,
        updated_at: now,
        messages: messages.to_vec(),
        children,
    };
    save(contest_dir, &session)
}

pub fn delete(contest_dir: &Path, name: &str) -> Result<()> {
    let dir = session_dir(contest_dir, name);
    if !dir.exists() {
        bail!("session '{name}' 不存在");
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

pub fn list(contest_dir: &Path) -> Result<Vec<SessionMeta>> {
    let dir = sessions_dir(contest_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let cur = current_name(contest_dir);
    let mut metas = Vec::new();
    for e in std::fs::read_dir(&dir)?.flatten() {
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name == CURRENT_FILE {
            continue;
        }
        let main_file = e.path().join(MAIN_FILE);
        if !main_file.is_file() {
            continue;
        }
        match std::fs::read_to_string(&main_file)
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
                if let Ok(meta) = e.metadata()
                    && let Ok(mtime) = meta.modified() {
                        let dt: DateTime<Utc> = mtime.into();
                        let is_cur = cur.as_deref() == Some(name.as_str());
                        metas.push(SessionMeta {
                            name,
                            updated_at: dt,
                            messages: 0,
                            current: is_cur,
                        });
                    }
            }
        }
    }
    metas.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    Ok(metas)
}

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

pub fn export_markdown(s: &Session) -> String {
    let mut out = format!(
        "# Session: {}\n\n创建：{}\n更新：{}\n消息数：{}\n\n",
        s.name,
        s.created_at.format("%Y-%m-%d %H:%M:%S"),
        s.updated_at.format("%Y-%m-%d %H:%M:%S"),
        s.messages.len()
    );
    for m in &s.messages {
        out.push_str(&format!("## {}\n\n", m.role));
        if let Some(c) = &m.content
            && !c.is_empty() {
                out.push_str(c);
                out.push_str("\n\n");
            }
        if let Some(tcs) = &m.tool_calls {
            for tc in tcs {
                out.push_str(&format!("> tool_call: `{}` `{}`\n", tc.function.name, tc.function.arguments));
            }
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_contest() -> PathBuf {
        let d = std::env::temp_dir().join(format!("prep_sess_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_load_list_roundtrip() {
        let c = tmp_contest();
        save_messages(&c, "s1", &[Message::system("sys"), Message::user("hi")]).unwrap();
        assert_eq!(current_name(&c).as_deref(), Some("s1"));
        let s = load(&c, "s1").unwrap();
        assert_eq!(s.name, "s1");
        assert_eq!(s.messages.len(), 2);
        assert!(s.children.is_empty());
        let metas = list(&c).unwrap();
        assert_eq!(metas.len(), 1);
        assert!(metas[0].current);
        assert_eq!(metas[0].messages, 2);
        std::fs::remove_dir_all(&c).ok();
    }

    #[test]
    fn sub_session_save_load() {
        let c = tmp_contest();
        save_messages(&c, "s1", &[Message::user("hi")]).unwrap();
        // 模拟 pending sub session
        push_pending_sub_session("solution".into(), vec![Message::user("sub task")], "写std".into());
        save_messages(&c, "s1", &[Message::user("hi"), Message { role: "assistant".into(), content: Some("result".into()), tool_calls: None, tool_call_id: None, reasoning: None }]).unwrap();
        let s = load(&c, "s1").unwrap();
        assert_eq!(s.children.len(), 1);
        assert_eq!(s.children[0].filename, "sub-1.json");
        assert_eq!(s.children[0].agent, "solution");
        // 加载子 session
        let sub = load_sub(&c, "s1", &s.children[0].filename).unwrap();
        assert_eq!(sub.agent, "solution");
        assert_eq!(sub.messages.len(), 1);
        std::fs::remove_dir_all(&c).ok();
    }

    #[test]
    fn auto_name_unique() {
        let c = tmp_contest();
        let n1 = auto_name(&c).unwrap();
        save_messages(&c, &n1, &[Message::user("x")]).unwrap();
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
    fn last_uses_current_pointer() {
        let c = tmp_contest();
        save_messages(&c, "a", &[Message::user("a")]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        save_messages(&c, "b", &[Message::user("b")]).unwrap();
        set_current(&c, "a").unwrap();
        let s = last(&c).unwrap().unwrap();
        assert_eq!(s.name, "a");
        clear_current(&c);
        let s = last(&c).unwrap().unwrap();
        assert_eq!(s.name, "b");
        std::fs::remove_dir_all(&c).ok();
    }

    #[test]
    fn export_markdown_contains_messages() {
        let s = Session {
            name: "t".into(), created_at: Utc::now(), updated_at: Utc::now(),
            messages: vec![Message::user("hello")], children: vec![],
        };
        let md = export_markdown(&s);
        assert!(md.contains("hello"));
    }

    fn assistant_with_tool_calls(ids: &[&str]) -> Message {
        Message {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(
                ids.iter()
                    .map(|id| crate::client::ToolCall {
                        id: id.to_string(),
                        kind: "function".into(),
                        function: crate::client::FunctionCall {
                            name: "bash".into(),
                            arguments: "{}".into(),
                        },
                    })
                    .collect(),
            ),
            tool_call_id: None,
            reasoning: None,
        }
    }

    #[test]
    fn repair_tool_history_fills_missing() {
        let mut msgs = vec![
            Message::user("hi"),
            assistant_with_tool_calls(&["a", "b", "c"]),
            Message::tool("r1".into(), "a".into()),
            Message::tool("r2".into(), "b".into()),
            // c 缺失
        ];
        repair_tool_history(&mut msgs);
        // c 的占位应插在 index 3（assistant 之后、其余 tool 之前也行，但语义顺序正确即可）
        let answered: Vec<&str> = msgs
            .iter()
            .filter(|m| m.role == "tool")
            .filter_map(|m| m.tool_call_id.as_deref())
            .collect();
        assert!(answered.contains(&"c"), "缺失的 c 应被补上: {answered:?}");
        assert_eq!(msgs.len(), 5);
    }

    #[test]
    fn repair_tool_history_noop_when_complete() {
        let mut msgs = vec![
            Message::user("hi"),
            assistant_with_tool_calls(&["a"]),
            Message::tool("r".into(), "a".into()),
        ];
        repair_tool_history(&mut msgs);
        assert_eq!(msgs.len(), 3);
    }
}
