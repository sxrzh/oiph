//! Web GUI 服务器：Axum HTTP + WebSocket，嵌入式前端。

use std::path::Path;
use std::sync::Arc;

use axum::{
    Router,
    extract::{
        Path as AxumPath, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::{IntoResponse, Json},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Mutex, mpsc};
use tower_http::services::ServeDir;

use crate::agent::{self, Role};
use crate::client::Message as ChatMessage;
use crate::project;
use crate::session as session_mod;
use crate::state::App;
use crate::term::CancelFlag;

// 前端由 ServeDir 从 frontend/dist/ 提供

struct ServerState {
    app: Arc<App>,
    messages: Arc<Mutex<Vec<ChatMessage>>>,
    current_session: Arc<Mutex<Option<String>>>,
    cancel: CancelFlag,
    /// 本回合累计 Token 用量（保存 session 时持久化并清零）。
    pending_usage: Arc<std::sync::Mutex<session_mod::TokenUsage>>,
    /// session 持久化的累计用量（状态栏基线）。
    saved_usage: Arc<std::sync::Mutex<session_mod::TokenUsage>>,
}

/// 当前总用量 = 持久化基线 + 本回合未保存部分。
fn total_usage(st: &ServerState) -> session_mod::TokenUsage {
    let saved = st.saved_usage.lock().unwrap().clone();
    let pending = st.pending_usage.lock().unwrap().clone();
    let mut t = saved;
    t.prompt_tokens += pending.prompt_tokens;
    t.completion_tokens += pending.completion_tokens;
    t.total_tokens += pending.total_tokens;
    t.cache_hit_tokens = match (t.cache_hit_tokens, pending.cache_hit_tokens) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    };
    t.cache_miss_tokens = match (t.cache_miss_tokens, pending.cache_miss_tokens) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    };
    t
}

pub async fn serve(app: Arc<App>, port: u16) -> anyhow::Result<()> {
    let contest_dir = app.contest_dir();

    // 加载上次 session（含持久化的 Token 用量基线）
    let (messages, current_session, saved_usage) = match &contest_dir {
        Some(c) => {
            let mut msgs = Vec::new();
            let mut sess = None;
            let mut usage = session_mod::TokenUsage::default();
            if let Ok(Some(s)) = session_mod::last(c) {
                let n = s.name.clone();
                msgs = s.messages;
                usage = s.usage;
                if msgs.is_empty() || msgs[0].role != "system" {
                    msgs.insert(0, agent::system_message_for(Role::Supervisor, &app));
                }
                sess = Some(n);
            } else {
                msgs.push(agent::system_message_for(Role::Supervisor, &app));
            }
            (msgs, sess, usage)
        }
        None => (
            vec![agent::system_message_for(Role::Supervisor, &app)],
            None,
            session_mod::TokenUsage::default(),
        ),
    };

    let state = Arc::new(ServerState {
        app,
        messages: Arc::new(Mutex::new(messages)),
        current_session: Arc::new(Mutex::new(current_session)),
        cancel: CancelFlag::new(),
        pending_usage: Arc::new(std::sync::Mutex::new(session_mod::TokenUsage::default())),
        saved_usage: Arc::new(std::sync::Mutex::new(saved_usage)),
    });

    let app_router = Router::new()
        .route("/api/contest", get(get_contest))
        .route("/api/problem/{pid}", get(get_problem))
        .route("/api/file", get(get_file).put(put_file))
        .route("/api/sessions", get(get_sessions))
        .route("/api/session/new", post(new_session))
        .route("/api/session/switch", post(switch_session))
        .route("/api/session/export", post(export_session))
        .route("/api/session/sub", get(get_sub_session))
        .route("/api/export/lemon", post(export_lemon))
        .route("/api/test", post(run_test))
        .route("/api/kb/search", post(kb_search))
        .route("/api/skills", get(list_skills))
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/frontend/dist")))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    println!("GUI 服务器启动：http://localhost:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app_router).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// REST handlers
// ---------------------------------------------------------------------------

async fn get_contest(State(st): State<Arc<ServerState>>) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    match &cdir {
        Some(d) => {
            let text = project::status_text(d);
            // 返回结构化的题目列表（前端用 ID 而非解析文本）
            let problems: Vec<Value> = match project::load_contest(d) {
                Ok(c) => c.loaded_problems.iter().map(|p| {
                    json!({
                        "id": p.id,
                        "name": p.name,
                        "type": p.problem_type.label(),
                        "source": p.source.label(),
                        "status": crate::model::GetStatus::get_status(p).label(),
                    })
                }).collect(),
                Err(_) => vec![],
            };
            Json(json!({ "contest_dir": d.display().to_string(), "status": text, "problems": problems }))
        }
        None => Json(json!({ "contest_dir": null, "status": "无比赛工程", "problems": [] })),
    }
}

async fn get_problem(
    State(st): State<Arc<ServerState>>,
    AxumPath(pid): AxumPath<String>,
) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    match &cdir {
        Some(d) => {
            let pdir = project::problem_dir(d, &pid);
            match project::load_problem(&pdir) {
                Ok(p) => {
                    // 组件状态
                    let comp_status = |s: &crate::model::ComponentStatus| -> Value {
                        match s {
                            crate::model::ComponentStatus::NotStarted => json!({"state":"not_started","label":"未开始","color":"gray"}),
                            crate::model::ComponentStatus::InProgress { progress, message } => json!({"state":"in_progress","label":format!("进行中 {}%",(*progress*100.0) as u32),"color":"yellow","progress":progress,"message":message}),
                            crate::model::ComponentStatus::Completed { timestamp } => json!({"state":"completed","label":"已完成","color":"green","timestamp":timestamp.format("%Y-%m-%d %H:%M").to_string()}),
                            crate::model::ComponentStatus::Failed { error } => json!({"state":"failed","label":"失败","color":"red","error":error}),
                        }
                    };
                    let sol_status: Vec<Value> = p.sols.iter().map(|s| json!({
                        "name": s.name,
                        "file": s.file,
                        "expected_verdict": s.expected.verdict.as_str(),
                        "expected_score": s.expected.score,
                        "status": comp_status(&s.status),
                    })).collect();
                    // 辅助程序文件列表
                    let aux_dir = pdir.join("auxiliary");
                    let mut aux_files: Vec<Value> = ["generator.cpp","validator.cpp","checker.cpp","interactive_lib.cpp"]
                        .iter().map(|f| {
                            let path = aux_dir.join(f);
                            let exists = path.exists();
                            json!({"name":f,"path":format!("auxiliary/{f}"),"exists":exists})
                        }).collect();
                    // 交互头文件
                    let inter_h = format!("{}.h", pid);
                    if aux_dir.join(&inter_h).exists() {
                        aux_files.push(json!({"name":&inter_h,"path":format!("auxiliary/{inter_h}"),"exists":true}));
                    }
                    // 测试点
                    let subtasks: Vec<Value> = p.subtasks.iter().map(|st| json!({
                        "score": st.score,
                        "type": match st.stype { crate::model::SubtaskType::Sum => "sum", crate::model::SubtaskType::Min => "min", crate::model::SubtaskType::Mul => "mul" },
                        "cases": st.cases,
                        "pretest": st.pretest,
                        "sample": st.sample,
                        "depend": st.depend,
                    })).collect();
                    // data_gen
                    let data_gen: std::collections::BTreeMap<&str, &str> = p.data_gen.iter().map(|(k,v)| (k.as_str(), v.as_str())).collect();

                    Json(json!({
                        "id": p.id,
                        "name": p.name,
                        "problem_type": p.problem_type.label(),
                        "source": p.source.label(),
                        "tags": p.tags,
                        "time_limit_ms": p.time_limit_ms,
                        "memory_limit_mb": p.memory_limit_mb,
                        "compile_flags": p.compile_flags,
                        "subtasks": subtasks,
                        "data_gen": data_gen,
                        "statement_status": comp_status(&p.statement),
                        "std_status": comp_status(&p.std.status),
                        "std_file": p.std.file,
                        "sols": sol_status,
                        "data_status": comp_status(&p.data.status),
                        "validator_status": comp_status(&p.validator.status),
                        "checker_status": comp_status(&p.checker.status),
                        "interactive_lib_status": p.interactive_lib.as_ref().map(|c| comp_status(&c.status)).unwrap_or(json!({"state":"not_started","label":"不适用","color":"gray"})),
                        "tutorial_status": comp_status(&p.tutorial),
                        "duplicate_check": p.duplicate_check.as_ref().map(|r| json!({
                            "found": r.found,
                            "matches": r.matches,
                        })),
                        "last_tested": p.last_tested.map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
                        "aux_files": aux_files,
                    }))
                }
                Err(e) => Json(json!({ "error": format!("{e:#}") })),
            }
        }
        None => Json(json!({ "error": "无比赛工程" })),
    }
}

#[derive(Deserialize)]
struct FileReq {
    path: String,
}

async fn get_file(State(st): State<Arc<ServerState>>, axum::extract::Query(req): axum::extract::Query<FileReq>) -> impl IntoResponse {
    let base = st.app.contest_dir().unwrap_or_else(|| st.app.root.clone());
    let p = base.join(&req.path);
    match std::fs::read_to_string(&p) {
        Ok(content) => Json(json!({ "content": content, "path": req.path })),
        Err(e) => Json(json!({ "error": format!("{e}") })),
    }
}

#[derive(Deserialize)]
struct WriteFileReq {
    path: String,
    content: String,
}

async fn put_file(
    State(st): State<Arc<ServerState>>,
    Json(req): Json<WriteFileReq>,
) -> impl IntoResponse {
    let base = st.app.contest_dir().unwrap_or_else(|| st.app.root.clone());
    let p = base.join(&req.path);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&p, &req.content) {
        Ok(_) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "error": format!("{e}") })),
    }
}

async fn get_sessions(State(st): State<Arc<ServerState>>) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    match &cdir {
        Some(d) => {
            let metas = session_mod::list(d).unwrap_or_default();
            let cur = session_mod::current_name(d);
            let list: Vec<Value> = metas
                .iter()
                .map(|m| {
                    json!({
                        "name": m.name,
                        "updated_at": m.updated_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                        "messages": m.messages,
                        "current": m.current,
                    })
                })
                .collect();
            Json(json!({ "sessions": list, "current": cur }))
        }
        None => Json(json!({ "sessions": [], "current": null })),
    }
}

#[derive(Deserialize)]
struct NewSessionReq {
    name: Option<String>,
}

async fn new_session(
    State(st): State<Arc<ServerState>>,
    Json(req): Json<NewSessionReq>,
) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    let Some(d) = &cdir else {
        return Json(json!({ "error": "无比赛工程" }));
    };
    // 保存当前
    save_session(&st).await;
    let name = match req.name {
        Some(n) => match session_mod::sanitize_name(&n) {
            Ok(s) => s,
            Err(e) => return Json(json!({ "error": format!("{e:#}") })),
        },
        None => match session_mod::auto_name(d) {
            Ok(n) => n,
            Err(e) => return Json(json!({ "error": format!("{e:#}") })),
        },
    };
    if session_mod::exists(d, &name) {
        return Json(json!({ "error": format!("会话 '{name}' 已存在") }));
    }
    let s = session_mod::Session {
        name: name.clone(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        messages: vec![agent::system_message_for(Role::Supervisor, &st.app)],
        children: vec![],
        usage: Default::default(),
    };
    let _ = session_mod::save(d, &s);
    *st.messages.lock().await = s.messages.clone();
    *st.current_session.lock().await = Some(name.clone());
    Json(json!({ "ok": true, "name": name }))
}

#[derive(Deserialize)]
struct SwitchReq {
    name: String,
}

async fn switch_session(
    State(st): State<Arc<ServerState>>,
    Json(req): Json<SwitchReq>,
) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    let Some(d) = &cdir else {
        return Json(json!({ "error": "无比赛工程" }));
    };
    if !session_mod::exists(d, &req.name) {
        return Json(json!({ "error": format!("会话 '{}' 不存在", req.name) }));
    }
    save_session(&st).await;
    match session_mod::load(d, &req.name) {
        Ok(s) => {
            let mut msgs = s.messages.clone();
            if msgs.is_empty() || msgs[0].role != "system" {
                msgs.insert(0, agent::system_message_for(Role::Supervisor, &st.app));
            }
            let _ = session_mod::set_current(d, &req.name);
            let messages_json: Vec<Value> = msgs
                .iter()
                .map(|m| {
                    json!({
                        "role": m.role,
                        "content": m.content,
                        "tool_calls": m.tool_calls,
                    })
                })
                .collect();
            let children_json: Vec<Value> = s.children.iter().map(|c| json!({
                "filename": c.filename,
                "agent": c.agent,
                "summary": c.summary,
            })).collect();
            *st.messages.lock().await = msgs;
            *st.current_session.lock().await = Some(req.name.clone());
            // 切换 session：用量基线切换为新 session 的持久化用量，清空 pending
            let usage_json = json!({
                "prompt_tokens": s.usage.prompt_tokens,
                "completion_tokens": s.usage.completion_tokens,
                "total_tokens": s.usage.total_tokens,
                "cache_hit_tokens": s.usage.cache_hit_tokens,
            });
            *st.saved_usage.lock().unwrap() = s.usage.clone();
            *st.pending_usage.lock().unwrap() = Default::default();
            Json(json!({ "ok": true, "name": req.name, "messages": messages_json, "children": children_json, "usage": usage_json }))
        }
        Err(e) => Json(json!({ "error": format!("{e:#}") })),
    }
}

#[derive(Deserialize)]
struct ExportReq {
    output: Option<String>,
}

async fn export_lemon(
    State(st): State<Arc<ServerState>>,
    Json(req): Json<ExportReq>,
) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    let Some(d) = &cdir else {
        return Json(json!({ "error": "无比赛工程" }));
    };
    let out = req.output.as_deref().map(Path::new);
    match crate::export_lemon::export(d, out) {
        Ok(path) => Json(json!({ "ok": true, "path": path.display().to_string() })),
        Err(e) => Json(json!({ "error": format!("{e:#}") })),
    }
}

#[derive(Deserialize)]
struct TestReq {
    problem: Option<String>,
}

async fn run_test(
    State(st): State<Arc<ServerState>>,
    Json(req): Json<TestReq>,
) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    let Some(d) = &cdir else {
        return Json(json!({ "error": "无比赛工程" }));
    };
    let reports = crate::test_runner::run_tests(d, req.problem.as_deref());
    let results: Vec<Value> = reports
        .iter()
        .map(|r| {
            json!({
                "problem_id": r.problem_id,
                "warnings": r.warnings,
                "errors": r.errors,
                "log": r.log,
            })
        })
        .collect();
    Json(json!({ "reports": results }))
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(st): State<Arc<ServerState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, st))
}

async fn handle_ws(mut socket: WebSocket, st: Arc<ServerState>) {
    let (tx_out, mut rx_out) = mpsc::unbounded_channel::<String>();
    let tx_out = Arc::new(tx_out);
    let tx_agent = tx_out.clone();
    let st_agent = st.clone();

    // 连接建立即推送当前累计用量（状态栏初始显示）
    {
        let total = total_usage(&st);
        let _ = tx_out.send(
            json!({
                "type": "usage",
                "usage": {
                    "prompt_tokens": total.prompt_tokens,
                    "completion_tokens": total.completion_tokens,
                    "total_tokens": total.total_tokens,
                    "cache_hit_tokens": total.cache_hit_tokens,
                },
            })
            .to_string(),
        );
    }

    // Agent 处理 task：从 channel 接收 chat 消息，运行 agent
    let (tx_chat, mut rx_chat) = mpsc::unbounded_channel::<String>();
    let agent_task = tokio::spawn(async move {
        while let Some(user_text) = rx_chat.recv().await {
            // 新回合：重置上一回合的中断标志
            st_agent.cancel.reset();
            crate::term::set_ws_sender(Some(tx_agent.as_ref().clone()));
            // 先写入用户消息
            {
                let mut msgs = st_agent.messages.lock().await;
                msgs[0] = agent::system_message_for(Role::Supervisor, &st_agent.app);
                msgs.push(ChatMessage::user(user_text));
            }
            // 确保 session 存在（第一条消息时创建，文件内含该消息），不存在则自动创建
            {
                let sess = st_agent.current_session.lock().await;
                if sess.is_none() {
                    drop(sess);
                    let mut sess = st_agent.current_session.lock().await;
                    if sess.is_none()
                        && let Some(cdir) = st_agent.app.contest_dir()
                            && let Ok(name) = session_mod::auto_name(&cdir) {
                                let msgs = st_agent.messages.lock().await.clone();
                                let s = session_mod::Session {
                                    name: name.clone(),
                                    created_at: chrono::Utc::now(),
                                    updated_at: chrono::Utc::now(),
                                    messages: msgs,
                                    children: vec![],
                                    usage: Default::default(),
                                };
                                let _ = session_mod::save(&cdir, &s);
                                *sess = Some(name.clone());
                                let _ = tx_agent.send(json!({
                                    "type": "session_created",
                                    "name": name,
                                }).to_string());
                            }
                }
            }
            let deps = st_agent.app.deps();
            // 增量保存：工具调用等每步变化都落盘
            let (save_tx, mut save_rx) = mpsc::unbounded_channel::<Vec<ChatMessage>>();
            let saver = st_agent.current_session.lock().await.clone().map(|name| {
                let app2 = st_agent.app.clone();
                tokio::spawn(async move {
                    while let Some(msgs) = save_rx.recv().await {
                        if let Some(d) = app2.contest_dir() {
                            let _ = session_mod::save_messages(&d, &name, &msgs, &session_mod::TokenUsage::default());
                        }
                    }
                })
            });
            let result = {
                let mut msgs = st_agent.messages.lock().await;
                agent::run_turn(
                    &deps,
                    &st_agent.app,
                    Role::Supervisor,
                    None,
                    &mut msgs,
                    true,
                    true,
                    &st_agent.cancel,
                    Some(&save_tx),
                )
                .await
            };
            drop(save_tx);
            if let Some(s) = saver {
                let _ = s.await;
            }
            crate::term::set_ws_sender(None);

            match result {
                Ok(turn_result) => {
                    // 累计到 pending，待保存时持久化
                    if let Some(u) = &turn_result.usage {
                        let mut pending = st_agent.pending_usage.lock().unwrap();
                        pending.prompt_tokens += u.prompt_tokens;
                        pending.completion_tokens += u.completion_tokens;
                        pending.total_tokens += u.total_tokens;
                        pending.cache_hit_tokens = match (pending.cache_hit_tokens, u.cache_hit_tokens) {
                            (Some(a), Some(b)) => Some(a + b),
                            (a, b) => a.or(b),
                        };
                        pending.cache_miss_tokens = match (pending.cache_miss_tokens, u.cache_miss_tokens) {
                            (Some(a), Some(b)) => Some(a + b),
                            (a, b) => a.or(b),
                        };
                    }
                    let total = total_usage(&st_agent);
                    let _ = tx_agent.send(
                        json!({
                            "type": "done",
                            "interrupted": turn_result.interrupted,
                            "usage": {
                                "prompt_tokens": total.prompt_tokens,
                                "completion_tokens": total.completion_tokens,
                                "total_tokens": total.total_tokens,
                                "cache_hit_tokens": total.cache_hit_tokens,
                            },
                        })
                        .to_string(),
                    );
                }
                Err(e) => {
                    let _ = tx_agent.send(
                        json!({ "type": "error", "message": format!("{e:#}") }).to_string(),
                    );
                }
            }
            save_session(&st_agent).await;
            // 保存后发送一次总用量（状态栏刷新为持久化后的值）
            let total = total_usage(&st_agent);
            let _ = tx_agent.send(
                json!({
                    "type": "usage",
                    "usage": {
                        "prompt_tokens": total.prompt_tokens,
                        "completion_tokens": total.completion_tokens,
                        "total_tokens": total.total_tokens,
                        "cache_hit_tokens": total.cache_hit_tokens,
                    },
                })
                .to_string(),
            );
            // 发送消息列表更新（含子 session 引用）
            let msgs = st_agent.messages.lock().await;
            let session_name = st_agent.current_session.lock().await.clone();
            let children: Vec<Value> = match &session_name {
                Some(n) => match session_mod::load(&st_agent.app.contest_dir().unwrap_or_else(|| st_agent.app.root.clone()), n) {
                    Ok(s) => s.children.iter().map(|c| json!({
                        "filename": c.filename,
                        "agent": c.agent,
                        "summary": c.summary,
                    })).collect(),
                    Err(_) => vec![],
                },
                None => vec![],
            };
            let messages_json: Vec<Value> = msgs
                .iter()
                .map(|m| json!({"role": m.role, "content": m.content, "tool_calls": m.tool_calls}))
                .collect();
            let _ = tx_agent.send(
                json!({ "type": "messages", "messages": messages_json, "children": children, "session_name": session_name }).to_string(),
            );
        }
    });

    // 主循环：WebSocket ↔ channel 双向转发
    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(req) = serde_json::from_str::<Value>(&text) {
                            match req["type"].as_str() {
                                Some("chat") => {
                                    let _ = tx_chat.send(req["text"].as_str().unwrap_or("").to_string());
                                }
                                Some("stop") => {
                                    st.cancel.cancel();
                                }
                                Some("ask_answer") => {
                                    // 问卷提交/取消：转发给等待中的 ask_user 工具
                                    st.app.send_ask_answer(req.clone());
                                }
                                Some("undo") | Some("redo") => {
                                    let is_undo = req["type"] == "undo";
                                    let cur_len = st.messages.lock().await.len();
                                    let result = if is_undo {
                                        st.app.snapshot_undo(cur_len)
                                    } else {
                                        st.app.snapshot_redo(cur_len)
                                    };
                                    let msg = match result {
                                        Ok(Some(point)) => {
                                            // 对话同步回退/恢复
                                            st.messages.lock().await.truncate(point.msg_len);
                                            // 持久化截断后的对话
                                            save_session(&st).await;
                                            format!(
                                                "已{}到快照 {}（对话已同步{}到 {} 条消息）",
                                                if is_undo { "回滚" } else { "重做" },
                                                &point.hash[..8.min(point.hash.len())],
                                                if is_undo { "回退" } else { "恢复" },
                                                point.msg_len
                                            )
                                        }
                                        Ok(None) => {
                                            if is_undo { "没有可回滚的快照" } else { "没有可重做的快照" }.to_string()
                                        }
                                        Err(e) => format!("{}失败：{e:#}", if is_undo { "回滚" } else { "重做" }),
                                    };
                                    let _ = tx_out.send(
                                        json!({ "type": "tool_result", "text": msg }).to_string(),
                                    );
                                    // 快照变化后刷新题目区 + 全量消息刷新
                                    let _ = tx_out.send(
                                        json!({ "type": "snapshot_done" }).to_string(),
                                    );
                                    let msgs = st.messages.lock().await;
                                    let messages_json: Vec<Value> = msgs
                                        .iter()
                                        .map(|m| json!({"role": m.role, "content": m.content, "tool_calls": m.tool_calls}))
                                        .collect();
                                    let _ = tx_out.send(
                                        json!({ "type": "messages", "messages": messages_json }).to_string(),
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            msg = rx_out.recv() => {
                if let Some(text) = msg
                    && socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
            }
        }
    }

    drop(tx_chat);
    agent_task.abort();
}

async fn save_session(st: &ServerState) {
    let cdir = st.app.contest_dir();
    let Some(d) = &cdir else { return };
    let msgs = st.messages.lock().await;
    let mut sess = st.current_session.lock().await;
    let name = match sess.clone() {
        Some(n) => n,
        None => match session_mod::auto_name(d) {
            Ok(n) => {
                // 写回状态，避免下一轮重复创建 session
                *sess = Some(n.clone());
                n
            }
            Err(_) => return,
        },
    };
    // 取出本回合待保存用量，持久化后清零 pending
    let add_usage = {
        let mut pending = st.pending_usage.lock().unwrap();
        std::mem::take(&mut *pending)
    };
    if session_mod::save_messages(d, &name, &msgs, &add_usage).is_ok() {
        *st.saved_usage.lock().unwrap() = total_usage(st);
    }
}

// ---------------------------------------------------------------------------
// 额外 API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ExportSessionReq {
    name: Option<String>,
}

async fn export_session(
    State(st): State<Arc<ServerState>>,
    Json(req): Json<ExportSessionReq>,
) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    let Some(d) = &cdir else {
        return Json(json!({ "error": "无比赛工程" }));
    };
    let name = match req.name {
        Some(n) => n,
        None => match st.current_session.lock().await.clone() {
            Some(n) => n,
            None => return Json(json!({ "error": "没有当前会话" })),
        },
    };
    match session_mod::load(d, &name) {
        Ok(s) => Json(json!({ "markdown": session_mod::export_markdown(&s) })),
        Err(e) => Json(json!({ "error": format!("{e:#}") })),
    }
}

#[derive(Deserialize)]
struct SubSessionReq {
    session: String,
    filename: String,
}

async fn get_sub_session(
    State(st): State<Arc<ServerState>>,
    axum::extract::Query(req): axum::extract::Query<SubSessionReq>,
) -> impl IntoResponse {
    let cdir = st.app.contest_dir();
    let Some(d) = &cdir else {
        return Json(json!({ "error": "无比赛工程" }));
    };
    match session_mod::load_sub(d, &req.session, &req.filename) {
        Ok(sub) => {
            let messages: Vec<Value> = sub.messages.iter().map(|m| json!({
                "role": m.role,
                "content": m.content,
                "tool_calls": m.tool_calls,
            })).collect();
            Json(json!({ "agent": sub.agent, "messages": messages }))
        }
        Err(e) => Json(json!({ "error": format!("{e:#}") })),
    }
}

#[derive(Deserialize)]
struct KbSearchReq {
    query: String,
}

async fn kb_search(
    State(st): State<Arc<ServerState>>,
    Json(req): Json<KbSearchReq>,
) -> impl IntoResponse {
    let cfg = st.app.tool_ctx(&st.app.contest_dir().unwrap_or_else(|| st.app.root.clone())).kb_ctx();
    match crate::kb::search(&cfg, &req.query, 4).await {
        Ok(out) => Json(json!({ "result": out })),
        Err(e) => Json(json!({ "error": format!("{e:#}") })),
    }
}

async fn list_skills(State(st): State<Arc<ServerState>>) -> impl IntoResponse {
    let roots = st.app.skill_roots();
    let found = crate::skills::discover(&roots);
    let list: Vec<Value> = found.iter().map(|s| {
        json!({ "name": s.name, "description": s.description })
    }).collect();
    Json(json!({ "skills": list }))
}
