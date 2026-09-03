//! 基础工具：bash、文件读写、web_search、fetch_url、kb_search、skills、testlib/checker。

use anyhow::Result;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

/// 子进程守卫：被 drop 时 kill 掉子进程，防止 cancel 后遗留孤儿进程。
struct ChildGuard {
    pid: Option<u32>,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            // 用 kill 杀掉整个进程组
            let _ = std::process::Command::new("kill")
                .arg("-9")
                .arg(pid.to_string())
                .output();
        }
    }
}

use crate::assets;
use crate::client::{FunctionDef, Tool};
use crate::kb;
use crate::skills;

pub const MAX_OUTPUT_CHARS: usize = 8000;
pub const MAX_FILE_CHARS: usize = 20000;

// ---------------------------------------------------------------------------
// 上下文
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ToolContext {
    pub workdir: PathBuf,
    /// 知识库目录（全局 ~/.oiph/kb 与工程 .oiph/kb）。
    pub kb_dirs: Vec<PathBuf>,
    /// skills 根目录（全局 ~/.oiph/skills 与工程 .oiph/skills）。
    pub skill_roots: Vec<PathBuf>,
    pub base_url: String,
    pub api_key: String,
    pub embed_model: Option<String>,
    /// 查重后端。
    pub dup_backend: crate::dupcheck::Backend,
}

impl ToolContext {
    pub fn kb_ctx(&self) -> kb::KbConfig {
        kb::KbConfig {
            dirs: self.kb_dirs.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            embed_model: self.embed_model.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

pub fn get_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("缺少或非字符串参数 '{key}'"))
}

pub fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

pub fn truncate(prefix: &str, s: &str) -> String {
    if s.chars().count() > MAX_OUTPUT_CHARS {
        let cut: String = s.chars().take(MAX_OUTPUT_CHARS).collect();
        format!("{prefix}{cut}...[已截断]")
    } else {
        format!("{prefix}{s}")
    }
}

pub fn resolve_path(ctx: &ToolContext, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.workdir.join(p)
    }
}

// ---------------------------------------------------------------------------
// 工具定义
// ---------------------------------------------------------------------------

pub fn definition(name: &str) -> Option<Tool> {
    Some(Tool {
        kind: "function".into(),
        function: match name {
            "bash" => FunctionDef {
                name: "bash".into(),
                description: "执行 bash 命令并返回合并的 stdout/stderr。工作目录为当前比赛目录。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "要执行的 bash 命令。" }
                    },
                    "required": ["command"]
                }),
            },
            "read_file" => FunctionDef {
                name: "read_file".into(),
                description: "读取指定文件的完整文本内容。相对路径基于当前比赛目录。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "文件路径。" }
                    },
                    "required": ["path"]
                }),
            },
            "write_file" => FunctionDef {
                name: "write_file".into(),
                description: "写入文本到文件，必要时创建父目录并覆盖已有内容。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "文件路径。" },
                        "content": { "type": "string", "description": "要写入的内容。" }
                    },
                    "required": ["path", "content"]
                }),
            },
            "web_search" => FunctionDef {
                name: "web_search".into(),
                description: "用 Bing 搜索网络，返回标题、URL、摘要。用于获取最新信息。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "搜索关键词。" }
                    },
                    "required": ["query"]
                }),
            },
            "fetch_url" => FunctionDef {
                name: "fetch_url".into(),
                description: "抓取指定 URL 的内容。HTML 会提取为纯文本，否则原样返回。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "要抓取的 URL。" }
                    },
                    "required": ["url"]
                }),
            },
            "kb_search" => FunctionDef {
                name: "kb_search".into(),
                description: "在用户知识库（RAG）中检索，返回最相关文本片段、来源与相似度。题面规范、题目来源等文档在此。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "检索查询。" },
                        "k": { "type": "integer", "description": "返回数量（1-8，默认 4）。" }
                    },
                    "required": ["query"]
                }),
            },
            "list_skills" => FunctionDef {
                name: "list_skills".into(),
                description: "列出可用 Skills（全局 ~/.oiph/skills 与工程 .oiph/skills 下的 skill 目录）。".into(),
                parameters: serde_json::json!({ "type": "object", "properties": {} }),
            },
            "load_skill" => FunctionDef {
                name: "load_skill".into(),
                description: "加载某个 Skill 的完整 SKILL.md 指令内容以便遵循。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "skill 名称（frontmatter 的 name）。" }
                    },
                    "required": ["name"]
                }),
            },
            "get_testlib" => FunctionDef {
                name: "get_testlib".into(),
                description: "将内置的 testlib.h 写到指定路径（默认 testlib.h），供辅助程序编译。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "目标相对路径，默认 'testlib.h'。" }
                    }
                }),
            },
            "get_checker" => FunctionDef {
                name: "get_checker".into(),
                description: "获取内置的常见 checker 模板源码并写到指定路径。可用模板见返回。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "checker 名称，如 wcmp/acmp/nyesno/rcmp 等。" },
                        "path": { "type": "string", "description": "目标相对路径，默认 'checker_<name>.cpp'。" }
                    },
                    "required": ["name"]
                }),
            },
            _ => return None,
        },
    })
}

/// 所有基础工具名（不含项目/子 Agent 工具）。
pub const BASE_TOOLS: &[&str] = &[
    "bash",
    "read_file",
    "write_file",
    "web_search",
    "fetch_url",
    "kb_search",
    "list_skills",
    "load_skill",
];

pub const AUX_TOOLS: &[&str] = &["get_testlib", "get_checker"];

// ---------------------------------------------------------------------------
// 派发
// ---------------------------------------------------------------------------

/// 派发基础工具。返回 None 表示不是基础工具。
pub async fn dispatch_base(ctx: &ToolContext, name: &str, args: &Value) -> Option<String> {
    Some(match name {
        "bash" => match run_bash(ctx, args).await {
            Ok(out) => out,
            Err(e) => format!("[bash 错误] {e:#}"),
        },
        "read_file" => match read_file(ctx, args).await {
            Ok(out) => out,
            Err(e) => format!("[read_file 错误] {e:#}"),
        },
        "write_file" => match write_file(ctx, args).await {
            Ok(out) => out,
            Err(e) => format!("[write_file 错误] {e:#}"),
        },
        "web_search" => match web_search(args).await {
            Ok(out) => out,
            Err(e) => format!("[web_search 错误] {e:#}"),
        },
        "fetch_url" => match fetch_url(args).await {
            Ok(out) => out,
            Err(e) => format!("[fetch_url 错误] {e:#}"),
        },
        "kb_search" => match kb_search(ctx, args).await {
            Ok(out) => out,
            Err(e) => format!("[kb_search 错误] {e:#}"),
        },
        "list_skills" => match list_skills(ctx).await {
            Ok(out) => out,
            Err(e) => format!("[list_skills 错误] {e:#}"),
        },
        "load_skill" => match load_skill(ctx, args).await {
            Ok(out) => out,
            Err(e) => format!("[load_skill 错误] {e:#}"),
        },
        "get_testlib" => match get_testlib(ctx, args).await {
            Ok(out) => out,
            Err(e) => format!("[get_testlib 错误] {e:#}"),
        },
        "get_checker" => match get_checker(ctx, args).await {
            Ok(out) => out,
            Err(e) => format!("[get_checker 错误] {e:#}"),
        },
        _ => return None,
    })
}

async fn run_bash(ctx: &ToolContext, args: &Value) -> Result<String> {
    let command = get_str(args, "command")?;
    let child = Command::new("bash")
        .arg("-c")
        .arg(&command)
        .current_dir(&ctx.workdir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动 bash 失败：{e}"))?;

    let pid = child.id();
    // 子进程守卫：future 被 drop 时杀掉子进程
    let _guard = ChildGuard { pid };

    let output = child.wait_with_output()
        .await
        .map_err(|e| anyhow::anyhow!("等待 bash 失败：{e}"))?;

    let mut combined = String::new();
    if !output.stdout.is_empty() {
        combined.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push_str("\n[stderr]\n");
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let status = output.status.code().unwrap_or(-1);
    let body = if combined.is_empty() {
        format!("[exit code {status}, 无输出]")
    } else {
        truncate(&format!("[exit code {status}]\n"), &combined)
    };
    Ok(body)
}

async fn read_file(ctx: &ToolContext, args: &Value) -> Result<String> {
    let path = get_str(args, "path")?;
    let p = resolve_path(ctx, &path);
    let bytes = tokio::fs::read(&p)
        .await
        .map_err(|e| anyhow::anyhow!("读取 '{path}' 失败：{e}"))?;
    let content = String::from_utf8_lossy(&bytes);
    if content.chars().count() > MAX_FILE_CHARS {
        let cut: String = content.chars().take(MAX_FILE_CHARS).collect();
        Ok(format!("{cut}...[已截断，文件较大]"))
    } else {
        Ok(content.into_owned())
    }
}

async fn write_file(ctx: &ToolContext, args: &Value) -> Result<String> {
    let path = get_str(args, "path")?;
    let content = get_str(args, "content")?;
    let p = resolve_path(ctx, &path);
    if let Some(parent) = p.parent()
        && !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow::anyhow!("创建父目录失败：{e}"))?;
        }
    tokio::fs::write(&p, &content)
        .await
        .map_err(|e| anyhow::anyhow!("写入 '{path}' 失败：{e}"))?;
    Ok(format!("已写入 {} 字节到 {path}", content.len()))
}

async fn web_search(args: &Value) -> Result<String> {
    let query = get_str(args, "query")?;
    let http = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/124.0.0.0 Safari/537.36",
        )
        .timeout(Duration::from_secs(20))
        .build()?;
    let resp = http
        .get("https://www.bing.com/search")
        .header("Accept-Language", "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7")
        .query(&[("q", &query)])
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("搜索请求失败：{e}"))?;
    let html = resp.text().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let document = Html::parse_document(&html);

    let algo_sel = Selector::parse("li.b_algo").map_err(|e| anyhow::anyhow!("{e}"))?;
    let link_sel = Selector::parse("h2 a").map_err(|e| anyhow::anyhow!("{e}"))?;
    let snip_sel = Selector::parse(r#"p[class*="b_lineclamp"], .b_caption p"#)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut results = Vec::new();
    for node in document.select(&algo_sel) {
        let Some(a) = node.select(&link_sel).next() else {
            continue;
        };
        let title = node_text(a);
        if title.is_empty() {
            continue;
        }
        let href = a.value().attr("href").unwrap_or("").to_string();
        let url = decode_bing_href(&href);
        let snippet = node
            .select(&snip_sel)
            .next()
            .map(node_text)
            .unwrap_or_default();
        results.push(format!("• {title}\n  {url}\n  {snippet}"));
        if results.len() >= 8 {
            break;
        }
    }

    if results.is_empty() {
        Ok(format!("未找到 '{query}' 的结果"))
    } else {
        Ok(format!(
            "搜索 '{query}' 的结果：\n\n{}",
            results.join("\n\n")
        ))
    }
}

async fn fetch_url(args: &Value) -> Result<String> {
    let url = get_str(args, "url")?;
    let http = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/124.0.0.0 Safari/537.36",
        )
        .timeout(Duration::from_secs(20))
        .build()?;
    let resp = http
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("请求失败：{e}"))?;
    let is_html = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|t| t.contains("text/html"))
        .unwrap_or(false);
    let text = resp.text().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let body = if is_html {
        let doc = Html::parse_document(&text);
        let sel = Selector::parse("body").map_err(|e| anyhow::anyhow!("{e}"))?;
        doc.select(&sel)
            .next()
            .map(|b| {
                b.text()
                    .map(|t| t.trim())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    } else {
        text
    };
    Ok(truncate("", &body))
}

async fn kb_search(ctx: &ToolContext, args: &Value) -> Result<String> {
    let query = get_str(args, "query")?;
    let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(4) as usize;
    kb::search(&ctx.kb_ctx(), &query, k).await
}

async fn list_skills(ctx: &ToolContext) -> Result<String> {
    let found = skills::discover(&ctx.skill_roots);
    if found.is_empty() {
        return Ok(format!(
            "没有可用 skills（全局 {} 与工程 .oiph/skills 下创建 <名>/SKILL.md 即可）",
            ctx.skill_roots
                .first()
                .map(|r| r.display().to_string())
                .unwrap_or_else(|| "~/.oiph/skills".into())
        ));
    }
    let mut out = String::from("可用 Skills：\n");
    for sk in &found {
        out.push_str(&format!("- {}: {}\n", sk.name, sk.description));
    }
    Ok(out)
}

async fn load_skill(ctx: &ToolContext, args: &Value) -> Result<String> {
    let name = get_str(args, "name")?;
    let content = skills::load_content(&ctx.skill_roots, &name)?;
    Ok(truncate("", &content))
}

async fn get_testlib(ctx: &ToolContext, args: &Value) -> Result<String> {
    let rel = opt_str(args, "path").unwrap_or_else(|| "testlib.h".into());
    if rel.contains("..") {
        anyhow::bail!("路径不能包含 '..'");
    }
    let p = resolve_path(ctx, &rel);
    if let Some(parent) = p.parent()
        && !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    tokio::fs::write(&p, assets::TESTLIB_H).await?;
    Ok(format!("已写入 testlib.h（{} 字节）到 {rel}", assets::TESTLIB_H.len()))
}

async fn get_checker(ctx: &ToolContext, args: &Value) -> Result<String> {
    let name = get_str(args, "name")?;
    let src = assets::checker_source(&name)
        .ok_or_else(|| anyhow::anyhow!("未知 checker '{name}'，可用：{}", assets::CHECKER_NAMES.join(", ")))?;
    let rel = opt_str(args, "path").unwrap_or_else(|| format!("checker_{name}.cpp"));
    if rel.contains("..") {
        anyhow::bail!("路径不能包含 '..'");
    }
    let p = resolve_path(ctx, &rel);
    if let Some(parent) = p.parent()
        && !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    tokio::fs::write(&p, src).await?;
    Ok(format!("已写入 checker '{name}'（{} 字节）到 {rel}", src.len()))
}

// ---------------------------------------------------------------------------
// Bing 解码辅助
// ---------------------------------------------------------------------------

fn node_text(el: ElementRef<'_>) -> String {
    el.text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_bing_href(href: &str) -> String {
    let fallback = || href.to_string();
    let Ok(parsed) = url::Url::parse(href) else {
        return fallback();
    };
    let Some(u) = parsed
        .query_pairs()
        .find_map(|(k, v)| (k == "u").then(|| v.into_owned()))
    else {
        return fallback();
    };
    let Some(b64) = u.strip_prefix("a1") else {
        return fallback();
    };
    match URL_SAFE_NO_PAD
        .decode(b64)
        .or_else(|_| {
            let normalized: String = b64
                .chars()
                .map(|c| match c {
                    '-' => '+',
                    '_' => '/',
                    other => other,
                })
                .collect();
            STANDARD.decode(normalized)
        })
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|decoded| decoded.starts_with("http"))
    {
        Some(url) => url,
        None => fallback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_ctx() -> ToolContext {
        let d = std::env::temp_dir().join("preparer_test_ctx");
        std::fs::create_dir_all(&d).ok();
        ToolContext {
            workdir: d,
            kb_dirs: vec![std::env::temp_dir().join("preparer-test-nonexistent-kb")],
            skill_roots: vec![std::env::temp_dir().join("preparer-test-nonexistent-skills")],
            base_url: "http://localhost:1/v1".into(),
            api_key: "test".into(),
            embed_model: None,
            dup_backend: crate::dupcheck::Backend::Cpret,
        }
    }

    #[tokio::test]
    async fn bash_runs_echo() {
        let ctx = test_ctx();
        let out = dispatch_base(&ctx, "bash", &json!({"command": "echo hello-preparer"}))
            .await
            .unwrap();
        assert!(out.contains("hello-preparer"), "got: {out}");
        assert!(out.contains("exit code 0"));
    }

    #[tokio::test]
    async fn write_then_read_file() {
        let ctx = test_ctx();
        let path = ctx.workdir.join("rt.txt");
        std::fs::remove_file(&path).ok();
        let w = dispatch_base(&ctx, "write_file", &json!({"path": "rt.txt", "content": "a\nb"}))
            .await
            .unwrap();
        assert!(w.contains("已写入"));
        let r = dispatch_base(&ctx, "read_file", &json!({"path": "rt.txt"}))
            .await
            .unwrap();
        assert_eq!(r, "a\nb");
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn unknown_tool_returns_none() {
        let ctx = test_ctx();
        assert!(dispatch_base(&ctx, "nope", &json!({})).await.is_none());
    }

    #[tokio::test]
    async fn kb_search_empty_kb() {
        let ctx = test_ctx();
        let out = dispatch_base(&ctx, "kb_search", &json!({"query": "anything"}))
            .await
            .unwrap();
        assert!(out.contains("空"), "got: {out}");
    }

    #[tokio::test]
    async fn get_testlib_writes_file() {
        let ctx = test_ctx();
        let _ = std::fs::remove_file(ctx.workdir.join("testlib.h"));
        let out = dispatch_base(&ctx, "get_testlib", &json!({}))
            .await
            .unwrap();
        assert!(out.contains("testlib.h"));
        assert!(ctx.workdir.join("testlib.h").exists());
        std::fs::remove_file(ctx.workdir.join("testlib.h")).ok();
    }

    #[test]
    fn definitions_include_base_tools() {
        for name in BASE_TOOLS {
            assert!(definition(name).is_some(), "缺少工具定义: {name}");
        }
    }

    #[test]
    fn decode_bing_href_decodes_u_param() {
        let href =
            "https://www.bing.com/ck/a?!&&p=abc123&u=a1aHR0cHM6Ly9ydXN0LWxhbmcub3JnLw&ntb=1";
        assert_eq!(decode_bing_href(href), "https://rust-lang.org/");
    }
}
