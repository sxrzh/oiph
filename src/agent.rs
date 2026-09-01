//! Agent 循环、工具定义与派发（项目工具、子 Agent 调度、桩工具）。

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use chrono::Utc;
use serde_json::Value;

use crate::client::{ChatUsage, Client, FunctionDef, Message, Tool};
use crate::model::{
    ComponentStatus, DuplicateCheckResult, JudgingStatus, ProblemSource, ProblemType,
    SolutionStatus, Verdict,
};
use crate::project;
use crate::prompts;
use crate::state::App;
use crate::term::{self, CancelFlag};
use crate::tools::{self, ToolContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Supervisor,
    Searching,
    Statement,
    Solution,
    Auxiliary,
}

pub struct AgentDeps<'a> {
    pub client: &'a Client,
    pub model: &'a str,
    pub max_steps: usize,
}

/// 单轮 Agent 的结果。
pub struct TurnResult {
    pub text: String,
    pub interrupted: bool,
    #[allow(dead_code)]
    pub usage: Option<ChatUsage>,
}

// ---------------------------------------------------------------------------
// 工具定义
// ---------------------------------------------------------------------------

pub const SUPERVISOR_TOOLS: &[&str] = &[
    "create_contest",
    "add_problem",
    "update_problem_meta",
    "get_problem",
    "get_project_status",
    "set_status",
    "add_solution",
    "duplicate_check",
    "check_data",
    "check_std",
    "check_solutions",
    "test_integrity",
    "call_searching_agent",
    "call_statement_agent",
    "call_solution_agent",
    "call_auxiliary_agent",
];

pub const SUBAGENT_TOOLS: &[&str] = &["get_problem"];

pub fn definition(name: &str) -> Option<Tool> {
    if let Some(t) = tools::definition(name) {
        return Some(t);
    }
    Some(Tool {
        kind: "function".into(),
        function: match name {
            "create_contest" => FunctionDef {
                name: "create_contest".into(),
                description: "在当前目录下创建一个新的比赛工程，并切换为当前比赛。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "比赛名称（也是目录名，不能含空白字符）。" }
                    },
                    "required": ["name"]
                }),
            },
            "add_problem" => FunctionDef {
                name: "add_problem".into(),
                description: "向当前比赛添加一道题目，创建目录结构与配置文件。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "题目目录名（英文/数字/连字符）。" },
                        "name": { "type": "string", "description": "题目名称。" },
                        "problem_type": { "type": "string", "description": "traditional/interactive_lib/interactive_io/answer_only/function" },
                        "source": { "type": "string", "description": "original/moved/adapted" }
                    },
                    "required": ["id"]
                }),
            },
            "update_problem_meta" => FunctionDef {
                name: "update_problem_meta".into(),
                description: "更新题目元信息：名称、标签、来源、类型、时限、空间限制。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则要求比赛只有一道题）。" },
                        "name": { "type": "string" },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "source": { "type": "string", "description": "original/moved/adapted" },
                        "problem_type": { "type": "string", "description": "traditional/interactive_lib/interactive_io/answer_only/function" },
                        "time_limit_ms": { "type": "integer" },
                        "memory_limit_mb": { "type": "integer" }
                    }
                }),
            },
            "get_problem" => FunctionDef {
                name: "get_problem".into(),
                description: "查看题目配置（YAML）与目录文件清单。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则要求比赛只有一道题）。" }
                    }
                }),
            },
            "get_project_status" => FunctionDef {
                name: "get_project_status".into(),
                description: "查看当前比赛与所有题目各组件状态。".into(),
                parameters: serde_json::json!({ "type": "object", "properties": {} }),
            },
            "set_status" => FunctionDef {
                name: "set_status".into(),
                description: "手动设置组件状态。component 取 statement/std/data/validator/checker/interactive_lib/tutorial/sols/sol:<name>。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则要求比赛只有一道题）。" },
                        "component": { "type": "string", "description": "组件名。" },
                        "state": { "type": "string", "description": "not_started/in_progress/completed/failed" },
                        "message": { "type": "string", "description": "in_progress 或 failed 时的说明。" },
                        "progress": { "type": "number", "description": "0-1 的进度，in_progress 时使用。" }
                    },
                    "required": ["component", "state"]
                }),
            },
            "add_solution" => FunctionDef {
                name: "add_solution".into(),
                description: "登记一个非 std 解法及其预期评测结果。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则要求比赛只有一道题）。" },
                        "name": { "type": "string", "description": "解法名，如 brute。" },
                        "file": { "type": "string", "description": "相对题目目录的路径，默认 solutions/<name>.cpp。" },
                        "expected_verdict": { "type": "string", "description": "AC/WA/TLE/MLE/RE/PARTIAL。" },
                        "expected_score": { "type": "number", "description": "预期得分。" }
                    },
                    "required": ["name", "expected_verdict"]
                }),
            },
            "duplicate_check" => FunctionDef {
                name: "duplicate_check".into(),
                description: "查找原题。默认通过 cpret.online 检索（可用 backend 参数切换为 yuantiji）。返回相似题目列表与相似度。相似度高时需向用户报告并询问是否继续。注意：cpret 会截断超过约 2048 tokens 的查询，应使用形式化题意（精简、突出数学本质）作为查询，避免冗长的背景故事。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则要求比赛只有一道题）。" },
                        "title": { "type": "string", "description": "题目标题或核心一句话描述。" },
                        "keywords": { "type": "string", "description": "查重关键词/题目描述摘要，将与 title 拼接为查询。" },
                        "k": { "type": "integer", "description": "返回数量上限（默认 20）。" },
                        "backend": { "type": "string", "description": "查重后端：cpret（默认）或 yuantiji。" }
                    },
                    "required": ["title"]
                }),
            },
            "check_data" => FunctionDef {
                name: "check_data".into(),
                description: "检查题目数据的正确性（目前为桩实现，统一返回通过）。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则要求比赛只有一道题）。" }
                    }
                }),
            },
            "check_std" => FunctionDef {
                name: "check_std".into(),
                description: "检查 std 的正确性（目前为桩实现，统一返回通过）。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则要求比赛只有一道题）。" }
                    }
                }),
            },
            "check_solutions" => FunctionDef {
                name: "check_solutions".into(),
                description: "检查各解法评测结果是否符合预期（目前为桩实现，统一返回符合）。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则要求比赛只有一道题）。" }
                    }
                }),
            },
            "test_integrity" => FunctionDef {
                name: "test_integrity".into(),
                description: "集成测试：编译辅助程序、造数据、验证、运行 std 和 sols、检查正确性。纯确定性过程。返回警告和错误信息。".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "problem": { "type": "string", "description": "题目 id（不指定则测试全部题目）。" }
                    }
                }),
            },
            "call_searching_agent" => sub_agent_def(
                "call_searching_agent",
                "调用 searching-agent：搜索冷门题目与资料，估计难度与知识点。task 需自包含。",
            ),
            "call_statement_agent" => sub_agent_def(
                "call_statement_agent",
                "调用 statement-agent：出题/写题面/改编题面/写题解。task 需自包含。",
            ),
            "call_solution_agent" => sub_agent_def(
                "call_solution_agent",
                "调用 solution-agent：设计算法写 std 及其他解法并预估评测结果。task 需自包含。",
            ),
            "call_auxiliary_agent" => sub_agent_def(
                "call_auxiliary_agent",
                "调用 auxiliary-agent：写 generator/checker/validator/interactive_lib 并造数据。task 需自包含。",
            ),
            _ => return None,
        },
    })
}

fn sub_agent_def(name: &'static str, desc: &'static str) -> FunctionDef {
    FunctionDef {
        name: name.into(),
        description: desc.into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string", "description": "子 Agent 的任务描述（自包含：含题目 id、要求、约束、上下文）。" },
                "problem": { "type": "string", "description": "相关题目 id（用于状态更新与工作定位）。" },
                "component": { "type": "string", "description": "任务对应的组件名（statement/std/sols/data/validator/checker/interactive_lib/tutorial），完成后自动更新该组件状态。" }
            },
            "required": ["task"]
        }),
    }
}

pub fn definitions_for(role: Role) -> Vec<Tool> {
    let mut names: Vec<&str> = tools::BASE_TOOLS.to_vec();
    match role {
        Role::Supervisor => names.extend(SUPERVISOR_TOOLS),
        Role::Searching | Role::Statement => names.extend_from_slice(SUBAGENT_TOOLS),
        Role::Solution => names.extend(
            SUBAGENT_TOOLS
                .iter()
                .copied()
                .chain(["add_solution", "set_status"]),
        ),
        Role::Auxiliary => {
            names.extend_from_slice(tools::AUX_TOOLS);
            names.extend(
                SUBAGENT_TOOLS
                    .iter()
                    .copied()
                    .chain(["set_status"]),
            );
        }
    }
    names.iter().filter_map(|n| definition(n)).collect()
}

/// 构造某角色的系统消息：基础系统提示词 + skills 清单（嵌入 name/description），
/// 子 Agent 额外附 RESULT 完成标志说明。
pub fn system_message_for(role: Role, app: &App) -> Message {
    let skills = crate::skills::discover(&app.skill_roots());
    let mut text = prompts::system_prompt(role).to_string();
    text.push_str(&crate::skills::prompt_section(&skills));
    if role != Role::Supervisor {
        text.push_str(prompts::RESULT_HINT);
    }
    Message::system(text)
}

// ---------------------------------------------------------------------------
// Agent 循环
// ---------------------------------------------------------------------------

/// 运行一个 Agent 的对话回合：调用模型、执行工具调用，直到模型给出最终回答。
/// 消息原地追加，跨回合累积。
///
/// - supervisor：工作目录动态取 App 的当前比赛目录（无则 root）。
/// - 子 Agent：使用固定的 `fixed_workdir`（比赛目录）。
/// - `stream_output`：是否实时打印模型输出内容（supervisor 为 true，子 Agent 为 false）。
/// - `cancel`：打断标志（双 Esc 触发）。
#[allow(clippy::too_many_arguments)]
pub async fn run_turn(
    deps: &AgentDeps<'_>,
    app: &App,
    role: Role,
    fixed_workdir: Option<&Path>,
    messages: &mut Vec<Message>,
    stream_content: bool,
    stream_reasoning: bool,
    cancel: &CancelFlag,
) -> Result<TurnResult> {
    let tool_defs = definitions_for(role);
    let on_content: fn(&str) = if stream_content { term::print_content } else { term::noop };
    let on_reasoning: fn(&str) = if stream_reasoning { term::print_reasoning } else { term::noop };
    let mut total_usage: Option<ChatUsage> = None;
    // searching-agent 的搜索调用计数（web_search + fetch_url），超过上限则停止
    let mut search_call_count: u32 = 0;
    const SEARCH_LIMIT: u32 = 30;

    for step in 1..=deps.max_steps {
        if cancel.is_cancelled() {
            return Ok(TurnResult {
                text: String::new(),
                interrupted: true,
                usage: total_usage,
            });
        }

        // 重置思维链缓冲
        if stream_reasoning {
            term::reset_reasoning_buf();
        }

        term::println_err(&format!(
            "--- [{}] step {step}: 调用模型 ---",
            prompts::role_name(role)
        ));
        let workdir = match fixed_workdir {
            Some(w) => w.to_path_buf(),
            None => app.contest_dir().unwrap_or_else(|| app.root.clone()),
        };
        let ctx = app.tool_ctx(&workdir);

        let result = deps
            .client
            .chat_stream(deps.model, messages, &tool_defs, cancel, on_content, on_reasoning)
            .await?;

        // 累加 usage
        if let Some(u) = &result.usage {
            total_usage = Some(match &mut total_usage {
                Some(acc) => {
                    acc.prompt_tokens += u.prompt_tokens;
                    acc.completion_tokens += u.completion_tokens;
                    acc.total_tokens += u.total_tokens;
                    acc.cache_hit_tokens = match (acc.cache_hit_tokens, u.cache_hit_tokens) {
                        (Some(a), Some(b)) => Some(a + b),
                        (a, b) => a.or(b),
                    };
                    acc.cache_miss_tokens = match (acc.cache_miss_tokens, u.cache_miss_tokens) {
                        (Some(a), Some(b)) => Some(a + b),
                        (a, b) => a.or(b),
                    };
                    acc.clone()
                }
                None => u.clone(),
            });
        }

        // 每步结束后实时显示累计用量
        if let Some(acc) = &total_usage {
            term::println_err(&crate::client::format_usage(deps.model, acc));
        }

        if result.interrupted {
            // 用户打断：保留已产生的 assistant 消息（content + tool_calls），但不包含 reasoning
            let mut msg = result.message;
            msg.reasoning = None;
            messages.push(msg);
            return Ok(TurnResult {
                text: String::new(),
                interrupted: true,
                usage: total_usage,
            });
        }

        let mut assistant_msg = result.message;
        if let Some(r) = assistant_msg.reasoning.as_deref()
            && !r.is_empty() {
                term::println_err(&format!("（思维链 {} 字符，不进入历史）", r.len()));
            }
        assistant_msg.reasoning = None;

        let tool_calls = assistant_msg.tool_calls.clone().unwrap_or_default();

        if tool_calls.is_empty() {
            let text = assistant_msg.content.clone().unwrap_or_default();
            messages.push(assistant_msg);
            return Ok(TurnResult {
                text,
                interrupted: false,
                usage: total_usage,
            });
        }

        messages.push(assistant_msg);

        for call in &tool_calls {
            if cancel.is_cancelled() {
                return Ok(TurnResult {
                    text: String::new(),
                    interrupted: true,
                    usage: total_usage,
                });
            }
            let name = &call.function.name;
            let args_raw = &call.function.arguments;
            let args: Value = serde_json::from_str(args_raw).unwrap_or_else(|e| {
                serde_json::json!({
                    "_parse_error": e.to_string(),
                    "_raw_arguments": args_raw,
                })
            });

            // searching-agent 搜索次数限制
            if role == Role::Searching && (name == "web_search" || name == "fetch_url") {
                search_call_count += 1;
                if search_call_count > SEARCH_LIMIT {
                    term::println_err(&format!(
                        "搜索次数已达上限 {SEARCH_LIMIT}，停止搜索"
                    ));
                    let result = format!(
                        "已达搜索上限（{SEARCH_LIMIT} 次网络请求）。请停止搜索，\
向 supervisor 报告未能找到的资源（std、测试数据、辅助程序等），\
由 solution-agent / auxiliary-agent 自行编写。\
在最终回答中列出已找到的资源和未找到的部分。"
                    );
                    messages.push(Message::tool(result, call.id.clone()));
                    continue;
                }
            }

            term::println_err(&format!("工具调用：{name}({})", args_summary(&args)));
            let dispatch_result = Box::pin(dispatch(role, app, &ctx, deps, cancel, name, &args)).await;
            term::println_err(&format!("-> {}", summary_line(&dispatch_result)));
            messages.push(Message::tool(dispatch_result, call.id.clone()));
        }
    }

    anyhow::bail!("达到最大步数 {}，任务未完成", deps.max_steps);
}

fn args_summary(args: &Value) -> String {
    if let Some(obj) = args.as_object() {
        let parts: Vec<String> = obj
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let v = if v.chars().count() > 60 {
                    let cut: String = v.chars().take(60).collect();
                    format!("{cut}…")
                } else {
                    v
                };
                format!("{k}=\"{v}\"")
            })
            .collect();
        parts.join(", ")
    } else {
        args.to_string()
    }
}

fn summary_line(s: &str) -> String {
    let first = s.lines().next().unwrap_or("").to_string();
    if first.chars().count() > 100 {
        let cut: String = first.chars().take(100).collect();
        format!("{cut}…")
    } else {
        first
    }
}

// ---------------------------------------------------------------------------
// 工具派发
// ---------------------------------------------------------------------------

pub async fn dispatch(
    role: Role,
    app: &App,
    ctx: &ToolContext,
    deps: &AgentDeps<'_>,
    cancel: &CancelFlag,
    name: &str,
    args: &Value,
) -> String {
    if role == Role::Supervisor {
        let r = match name {
            "create_contest" => Some(tool_create_contest(app, ctx, args).await),
            "add_problem" => Some(tool_add_problem(ctx, args).await),
            "update_problem_meta" => Some(tool_update_problem_meta(ctx, args).await),
            "get_project_status" => Some(tool_get_project_status(ctx).await),
            "duplicate_check" => Some(tool_duplicate_check(ctx, args).await),
            "check_data" => Some(tool_check(ctx, args, "data").await),
            "check_std" => Some(tool_check(ctx, args, "std").await),
            "check_solutions" => Some(tool_check(ctx, args, "sols").await),
            "test_integrity" => Some(tool_test_integrity(ctx, args).await),
            "call_searching_agent" => Some(call_sub_agent(Role::Searching, app, ctx, deps, cancel, args).await),
            "call_statement_agent" => Some(call_sub_agent(Role::Statement, app, ctx, deps, cancel, args).await),
            "call_solution_agent" => Some(call_sub_agent(Role::Solution, app, ctx, deps, cancel, args).await),
            "call_auxiliary_agent" => Some(call_sub_agent(Role::Auxiliary, app, ctx, deps, cancel, args).await),
            _ => None,
        };
        if let Some(r) = r {
            return r;
        }
    }

    // 通用项目工具（supervisor 与子 Agent 共用）
    match name {
        "get_problem" => return tool_get_problem(ctx, args).await,
        "set_status" => return tool_set_status(ctx, args).await,
        "add_solution" => return tool_add_solution(ctx, args).await,
        _ => {}
    }

    // 基础工具
    match tools::dispatch_base(ctx, name, args).await {
        Some(s) => s,
        None => format!("[错误] 未知工具：{name}"),
    }
}

// ---------------------------------------------------------------------------
// 项目工具实现
// ---------------------------------------------------------------------------

fn err_str(e: anyhow::Error) -> String {
    format!("[错误] {e:#}")
}

async fn tool_create_contest(app: &App, ctx: &ToolContext, args: &Value) -> String {
    let name = match tools::get_str(args, "name") {
        Ok(n) => n,
        Err(e) => return err_str(e),
    };
    if name.contains(char::is_whitespace) || name.contains('/') || name.contains("..") {
        return "[错误] 比赛名称不能包含空白字符、路径分隔符或 '..'".into();
    }
    if project::is_contest_dir(&ctx.workdir) {
        return "[错误] 当前工作目录已是比赛工程目录，无需重复创建".into();
    }
    let dir = ctx.workdir.clone();
    match project::init_contest(&dir, &name) {
        Ok(_) => {
            app.set_contest_dir(Some(dir.clone()));
            format!("已在当前目录创建比赛工程：{}（目录：{}）", name, dir.display())
        }
        Err(e) => err_str(e),
    }
}

async fn tool_add_problem(ctx: &ToolContext, args: &Value) -> String {
    let id = match tools::get_str(args, "id") {
        Ok(i) => i,
        Err(e) => return err_str(e),
    };
    let name = tools::opt_str(args, "name");
    let ptype = tools::opt_str(args, "problem_type").and_then(|s| parse_problem_type(&s));
    let source = tools::opt_str(args, "source").and_then(|s| parse_source(&s));
    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    match project::add_problem(
        &contest_dir,
        project::NewProblem {
            id: &id,
            name: name.as_deref(),
            problem_type: ptype,
            source,
        },
    ) {
        Ok(p) => format!(
            "已添加题目 {}（{}，{}，{}）",
            p.id,
            p.name,
            p.problem_type.label(),
            p.source.label()
        ),
        Err(e) => err_str(e),
    }
}

async fn tool_update_problem_meta(ctx: &ToolContext, args: &Value) -> String {
    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    let pid = match project::resolve_problem_id(&contest_dir, tools::opt_str(args, "problem").as_deref()) {
        Ok(p) => p,
        Err(e) => return err_str(e),
    };
    let name = tools::opt_str(args, "name");
    let tags: Option<Vec<String>> = args
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let source = tools::opt_str(args, "source").and_then(|s| parse_source(&s));
    let ptype = tools::opt_str(args, "problem_type").and_then(|s| parse_problem_type(&s));
    let tl = args.get("time_limit_ms").and_then(|v| v.as_u64());
    let ml = args.get("memory_limit_mb").and_then(|v| v.as_u64());
    match project::set_problem_meta(
        &contest_dir,
        &pid,
        project::ProblemMeta {
            name,
            tags,
            source,
            problem_type: ptype,
            time_limit_ms: tl,
            memory_limit_mb: ml,
        },
    ) {
        Ok(()) => format!("已更新题目 {pid} 的元信息"),
        Err(e) => err_str(e),
    }
}

async fn tool_get_problem(ctx: &ToolContext, args: &Value) -> String {
    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    let pid = match project::resolve_problem_id(&contest_dir, tools::opt_str(args, "problem").as_deref()) {
        Ok(p) => p,
        Err(e) => return err_str(e),
    };
    let pdir = project::problem_dir(&contest_dir, &pid);
    match project::load_problem(&pdir) {
        Ok(p) => {
            let yaml = serde_yaml::to_string(&p).unwrap_or_default();
            format!(
                "题目 {pid} 配置：\n```yaml\n{yaml}```\n\n文件清单：\n{}",
                project::problem_files_listing(&pdir)
            )
        }
        Err(e) => err_str(e),
    }
}

async fn tool_get_project_status(ctx: &ToolContext) -> String {
    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    project::status_text(&contest_dir)
}

async fn tool_set_status(ctx: &ToolContext, args: &Value) -> String {
    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    let pid = match project::resolve_problem_id(&contest_dir, tools::opt_str(args, "problem").as_deref()) {
        Ok(p) => p,
        Err(e) => return err_str(e),
    };
    let component = match tools::get_str(args, "component") {
        Ok(c) => c,
        Err(e) => return err_str(e),
    };
    let state = match tools::get_str(args, "state") {
        Ok(s) => s,
        Err(e) => return err_str(e),
    };
    let message = tools::opt_str(args, "message");
    let progress = args.get("progress").and_then(|v| v.as_f64()).map(|f| f as f32);
    let status = match state.to_lowercase().as_str() {
        "not_started" | "notstarted" => ComponentStatus::NotStarted,
        "in_progress" | "inprogress" => ComponentStatus::in_progress(
            progress.unwrap_or(0.1),
            message.unwrap_or_default(),
        ),
        "completed" | "done" => ComponentStatus::completed_now(),
        "failed" | "error" => ComponentStatus::failed(message.unwrap_or_else(|| "未说明原因".into())),
        other => return format!("[错误] 未知状态 '{other}'（可用 not_started/in_progress/completed/failed）"),
    };
    match project::set_component_status(&contest_dir, &pid, &component, status) {
        Ok(_) => format!("已设置题目 {pid} 的 {component} 状态"),
        Err(e) => err_str(e),
    }
}

async fn tool_add_solution(ctx: &ToolContext, args: &Value) -> String {
    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    let pid = match project::resolve_problem_id(&contest_dir, tools::opt_str(args, "problem").as_deref()) {
        Ok(p) => p,
        Err(e) => return err_str(e),
    };
    let name = match tools::get_str(args, "name") {
        Ok(n) => n,
        Err(e) => return err_str(e),
    };
    let verdict_s = match tools::get_str(args, "expected_verdict") {
        Ok(v) => v,
        Err(e) => return err_str(e),
    };
    let Some(verdict) = Verdict::parse(&verdict_s) else {
        return format!("[错误] 未知评测结果 '{verdict_s}'（可用 AC/WA/TLE/MLE/RE/PARTIAL）");
    };
    let score = args.get("expected_score").and_then(|v| v.as_f64());
    let file = tools::opt_str(args, "file").unwrap_or_else(|| format!("solutions/{name}.cpp"));
    let sol = SolutionStatus {
        name: name.clone(),
        file: Some(file),
        expected: JudgingStatus {
            verdict,
            score,
        },
        status: ComponentStatus::NotStarted,
    };
    match project::add_solution(&contest_dir, &pid, sol) {
        Ok(()) => format!(
            "已登记解法 {name}（预期 {} {}）",
            verdict.as_str(),
            score.map(|s| format!("{s}")).unwrap_or_else(|| "-".into())
        ),
        Err(e) => err_str(e),
    }
}

async fn tool_duplicate_check(ctx: &ToolContext, args: &Value) -> String {
    let title = match tools::get_str(args, "title") {
        Ok(t) => t,
        Err(e) => return err_str(e),
    };
    let keywords = tools::opt_str(args, "keywords").unwrap_or_default();
    let k = args.get("k").and_then(|v| v.as_u64());
    let backend = tools::opt_str(args, "backend")
        .and_then(|s| crate::dupcheck::Backend::parse(&s).ok())
        .unwrap_or(ctx.dup_backend);
    let query = if keywords.is_empty() {
        title.clone()
    } else {
        format!("{title} {keywords}")
    };

    let contest_dir = current_contest(ctx).ok();
    let pid = contest_dir
        .as_deref()
        .and_then(|d| project::resolve_problem_id(d, tools::opt_str(args, "problem").as_deref()).ok());

    let results = match crate::dupcheck::search(&query, k, backend).await {
        Ok(r) => r,
        Err(e) => {
            // 请求失败时仍记录查重已尝试
            if let (Some(d), Some(p)) = (&contest_dir, &pid) {
                let _ = project::set_duplicate_check(
                    d,
                    p,
                    DuplicateCheckResult {
                        found: false,
                        matches: vec![],
                        checked_at: Utc::now(),
                        note: Some(format!("{} 请求失败：{e:#}", backend.as_str())),
                    },
                );
            }
            return format!("查重请求失败（{}）：{e:#}\n（已记录，请稍后重试）", backend.as_str());
        }
    };

    let found = crate::dupcheck::is_likely_duplicate(&results);
    let top: Vec<&crate::dupcheck::SearchResult> = results.iter().take(5).collect();

    let matches: Vec<String> = top
        .iter()
        .map(|r| format!("{}（{}，cos={:.3}）{}", r.title, r.src, r.cos, r.url))
        .collect();

    if let (Some(d), Some(p)) = (&contest_dir, &pid) {
        let _ = project::set_duplicate_check(
            d,
            p,
            DuplicateCheckResult {
                found,
                matches: matches.clone(),
                checked_at: Utc::now(),
                note: Some(format!(
                    "通过 {} 检索，返回 {} 条结果",
                    backend.as_str(),
                    results.len()
                )),
            },
        );
    }

    if results.is_empty() {
        return format!("查重结果：未找到原题。\n查询：{query}");
    }

    let mut out = format!(
        "查重结果：通过 {} 找到 {} 条相似题目{}\n查询：{query}\n\n",
        backend.as_str(),
        results.len(),
        if found { "（首条相似度较高，疑似原题）" } else { "" },
    );
    for (i, r) in top.iter().enumerate() {
        out.push_str(&format!(
            "{}. {}（来源：{}，相似度 {:.3}）\n   {}\n",
            i + 1,
            r.title,
            r.src,
            r.cos,
            r.url,
        ));
        // 题面预览（优先 t0 重写版，其次 original）
        let preview_src = r.t0.as_deref().or(r.original.as_deref()).unwrap_or("");
        if !preview_src.is_empty() {
            let preview: String = preview_src.chars().take(200).collect();
            out.push_str(&format!("   摘要：{}{}\n", preview, if preview_src.chars().count() > 200 { "..." } else { "" }));
        }
    }
    if found {
        out.push_str("\n⚠ 首条结果相似度 ≥ 0.85，疑似原题。请向用户报告并询问是否继续出题。");
    } else {
        out.push_str("\n相似度均较低，暂未发现明显原题。请向用户报告结果。");
    }
    out
}

async fn tool_check(ctx: &ToolContext, args: &Value, component: &str) -> String {
    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    let pid = match project::resolve_problem_id(&contest_dir, tools::opt_str(args, "problem").as_deref()) {
        Ok(p) => p,
        Err(e) => return err_str(e),
    };
    let done = ComponentStatus::completed_now();
    let r = match component {
        "data" => project::set_component_status(&contest_dir, &pid, "data", done.clone()),
        "std" => project::set_component_status(&contest_dir, &pid, "std", done.clone()),
        "sols" => project::set_component_status(&contest_dir, &pid, "sols", done.clone()),
        _ => unreachable!(),
    };
    match r {
        Ok(_) => format!(
            "检查通过（桩）：{} 检查尚未接入 tuack-ng 等真实工具，暂统一返回 true。",
            match component {
                "data" => "数据",
                "std" => "std",
                _ => "解法评测结果",
            }
        ),
        Err(e) => err_str(e),
    }
}

async fn tool_test_integrity(ctx: &ToolContext, args: &Value) -> String {
    let problem = tools::opt_str(args, "problem");
    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    let reports = crate::test_runner::run_tests(&contest_dir, problem.as_deref());
    let mut out = String::new();
    for report in &reports {
        out.push_str(&report.to_string_report());
        out.push('\n');
    }
    let total_errors: usize = reports.iter().map(|r| r.errors.len()).sum();
    let total_warnings: usize = reports.iter().map(|r| r.warnings.len()).sum();
    if total_errors == 0 && total_warnings == 0 {
        out.push_str("所有题目集成测试通过，无警告无错误。");
    } else {
        out.push_str(&format!("共 {} 个错误、{} 个警告。", total_errors, total_warnings));
    }
    out
}

// ---------------------------------------------------------------------------
// 子 Agent 调用
// ---------------------------------------------------------------------------

fn parse_result(text: &str) -> bool {
    let last = text.lines().rev().find(|l| !l.trim().is_empty());
    match last {
        Some(l) => {
            let l = l.trim();
            if let Some(rest) = l.strip_prefix("RESULT:") {
                !rest.trim().to_uppercase().starts_with("FAILED")
            } else {
                true
            }
        }
        None => true,
    }
}

async fn call_sub_agent(
    role: Role,
    app: &App,
    ctx: &ToolContext,
    deps: &AgentDeps<'_>,
    cancel: &CancelFlag,
    args: &Value,
) -> String {
    let task = match tools::get_str(args, "task") {
        Ok(t) => t,
        Err(e) => return err_str(e),
    };
    let problem = tools::opt_str(args, "problem");
    let component = tools::opt_str(args, "component");

    let contest_dir = match current_contest(ctx) {
        Ok(d) => d,
        Err(e) => return err_str(e),
    };
    let pid = match project::resolve_problem_id(&contest_dir, problem.as_deref()) {
        Ok(p) => p,
        Err(e) => return err_str(e),
    };

    // 开始前置 InProgress
    if let Some(comp) = &component {
        let _ = project::set_component_status(
            &contest_dir,
            &pid,
            comp,
            ComponentStatus::in_progress(0.05, format!("{} 执行中", prompts::role_name(role))),
        );
    }

    let mut messages = vec![
        system_message_for(role, app),
        Message::user(format!(
            "任务（题目 id：{pid}，比赛目录：{}）：\n{task}",
            contest_dir.display()
        )),
    ];

    let outcome = run_turn(
        deps,
        app,
        role,
        Some(&contest_dir),
        &mut messages,
        false,  // 子 Agent 不实时打印内容
        true,   // 子 Agent 显示思维链
        cancel, // 透传 supervisor 的打断信号
    )
    .await;

    let (final_text, ok) = match outcome {
        Ok(result) => {
            if result.interrupted {
                ("（子 Agent 被打断）".into(), false)
            } else {
                let ok = parse_result(&result.text);
                (result.text, ok)
            }
        }
        Err(e) => (format!("子 Agent 出错：{e:#}"), false),
    };

    if let Some(comp) = &component {
        let status = if ok {
            ComponentStatus::completed_now()
        } else {
            let msg: String = final_text.chars().take(300).collect();
            ComponentStatus::failed(msg)
        };
        let _ = project::set_component_status(&contest_dir, &pid, comp, status);
    }

    format!(
        "=== {} agent 结果（{}）===\n{}\n\n--- 对话记录（摘要） ---\n{}",
        prompts::role_name(role),
        if ok { "成功" } else { "失败" },
        final_text,
        transcript(&messages)
    )
}

fn transcript(messages: &[Message]) -> String {
    const MSG_CAP: usize = 400;
    const TOTAL_CAP: usize = 8000;
    let mut out = String::new();
    for m in messages {
        if m.role == "assistant"
            && let Some(c) = &m.content {
                if c.trim().is_empty() {
                    continue;
                }
                let mut line: String = c.chars().take(MSG_CAP).collect();
                if c.chars().count() > MSG_CAP {
                    line.push_str("…[截断]");
                }
                out.push_str(&format!("[assistant] {line}\n\n"));
                if out.chars().count() > TOTAL_CAP {
                    break;
                }
            }
    }
    if out.is_empty() {
        "（无）".into()
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// 上下文辅助
// ---------------------------------------------------------------------------

fn current_contest(ctx: &ToolContext) -> Result<PathBuf> {
    if project::is_contest_dir(&ctx.workdir) {
        return Ok(ctx.workdir.clone());
    }
    Err(anyhow!(
        "当前没有比赛工程（目录 {} 不是比赛目录），请先 create_contest 或使用 /contest use",
        ctx.workdir.display()
    ))
}

// ---------------------------------------------------------------------------
// 参数解析
// ---------------------------------------------------------------------------

fn parse_problem_type(s: &str) -> Option<ProblemType> {
    let t = s.trim().to_lowercase();
    Some(match t.as_str() {
        "traditional" | "传统" | "传统题" => ProblemType::Traditional,
        "interactive_lib" | "interactivlib" | "函数交互" => ProblemType::InteractiveLib,
        "interactive_io" | "interactiveio" | "io 交互" | "io交互" => ProblemType::InteractiveIO,
        "answer_only" | "answeronly" | "提交答案" => ProblemType::AnswerOnly,
        "function" | "函数题" => ProblemType::Function,
        _ => return None,
    })
}

fn parse_source(s: &str) -> Option<ProblemSource> {
    let t = s.trim().to_lowercase();
    Some(match t.as_str() {
        "original" | "原创" => ProblemSource::Original,
        "moved" | "搬运" => ProblemSource::Moved,
        "adapted" | "改编" => ProblemSource::Adapted,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(root: PathBuf) -> App {
        App::new(
            root,
            "http://localhost:1/v1".into(),
            "k".into(),
            None,
            "m".into(),
            10,
            crate::dupcheck::Backend::Cpret,
        )
        .unwrap()
    }

    fn deps(client: &Client) -> AgentDeps<'_> {
        AgentDeps {
            client,
            model: "m",
            max_steps: 10,
        }
    }

    #[test]
    fn parse_result_ok_and_failed() {
        assert!(parse_result("完成\nRESULT: OK"));
        assert!(!parse_result("RESULT: FAILED: 编译不过"));
        assert!(parse_result("没有标志也算成功"));
    }

    #[test]
    fn role_definitions_contain_calls_for_supervisor() {
        let defs = definitions_for(Role::Supervisor);
        let names: Vec<&str> = defs.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"call_solution_agent"));
        assert!(names.contains(&"check_data"));
        assert!(names.contains(&"bash"));
    }

    #[test]
    fn subagent_definitions_exclude_supervisor_tools() {
        let defs = definitions_for(Role::Solution);
        let names: Vec<&str> = defs.iter().map(|t| t.function.name.as_str()).collect();
        assert!(!names.contains(&"call_statement_agent"));
        assert!(names.contains(&"get_problem"));
        assert!(names.contains(&"add_solution"));
    }

    #[test]
    fn parse_types_and_sources() {
        assert_eq!(parse_problem_type("traditional"), Some(ProblemType::Traditional));
        assert_eq!(parse_problem_type("函数交互"), Some(ProblemType::InteractiveLib));
        assert_eq!(parse_source("moved"), Some(ProblemSource::Moved));
        assert_eq!(parse_source("改编"), Some(ProblemSource::Adapted));
    }

    #[tokio::test]
    async fn project_tools_work_end_to_end() {
        let root = std::env::temp_dir().join(format!("preparer_test_agent_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let app = test_app(root.clone());
        let client = Client::new("http://localhost:1/v1".into(), "k".into()).unwrap();
        let d = deps(&client);
        let cancel = term::CancelFlag::new();
        let ctx = app.tool_ctx(&root);

        let out = dispatch(Role::Supervisor, &app, &ctx, &d, &cancel, "create_contest", &serde_json::json!({"name": "mycontest"})).await;
        assert!(out.contains("创建比赛"), "got: {out}");
        assert_eq!(app.contest_dir(), Some(root.clone()));
        let cdir = root.clone();
        let cctx = app.tool_ctx(&cdir);

        let out = dispatch(Role::Supervisor, &app, &cctx, &d, &cancel, "add_problem", &serde_json::json!({"id": "a", "name": "A+B", "problem_type": "traditional", "source": "original"})).await;
        assert!(out.contains("已添加题目"), "got: {out}");

        let out = dispatch(Role::Supervisor, &app, &cctx, &d, &cancel, "add_solution", &serde_json::json!({"name": "brute", "expected_verdict": "WA", "expected_score": 30.0})).await;
        assert!(out.contains("已登记解法"), "got: {out}");

        let out = dispatch(Role::Supervisor, &app, &cctx, &d, &cancel, "check_std", &serde_json::json!({"problem": "a"})).await;
        assert!(out.contains("检查通过"), "got: {out}");

        let out = dispatch(Role::Supervisor, &app, &cctx, &d, &cancel, "duplicate_check", &serde_json::json!({"problem": "a", "title": "A+B"})).await;
        assert!(out.contains("查重") || out.contains("失败"), "got: {out}");

        let p = project::load_problem(&cdir.join("a")).unwrap();
        assert!(p.std.status.is_terminal_ok());
        assert!(p.duplicate_check.is_some());

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn tools_require_contest() {
        let root = std::env::temp_dir().join(format!("preparer_test_noct_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let app = test_app(root.clone());
        let client = Client::new("http://localhost:1/v1".into(), "k".into()).unwrap();
        let d = deps(&client);
        let cancel = term::CancelFlag::new();
        let ctx = app.tool_ctx(&root);
        let out = dispatch(Role::Supervisor, &app, &ctx, &d, &cancel, "check_data", &serde_json::json!({})).await;
        assert!(out.contains("错误"), "got: {out}");
        std::fs::remove_dir_all(&root).ok();
    }
}
