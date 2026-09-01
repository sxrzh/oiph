//! OI 模拟赛组题助手：多 Agent 系统 CLI。
//!
//! 用法：
//! - `preparer`：进入交互式对话（supervisor agent），在比赛目录下自动加载上次 session。
//! - `preparer "任务描述"`：单次任务（非交互模式读 stdin 多行任务）。
//! - `preparer status`：打印当前工程状态。
//! - `preparer kb add|list|clear|search`：管理知识库（全局 ~/.oiph/kb 或工程 .oiph/kb）。
//! - `preparer skill list|show|add|delete`：管理 skills。
//! - `preparer session list|new|use|delete|export|show`：管理会话。
//! - 交互式下支持 `/` 开头的本地指令，见 `/help`。

mod agent;
mod assets;
mod client;
mod dupcheck;
mod export_lemon;
mod kb;
mod model;
mod paths;
mod project;
mod prompts;
mod session;
mod skills;
mod state;
mod term;
mod test_runner;
mod tools;

use std::collections::VecDeque;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use rustyline::Editor;
use serde::{Deserialize, Serialize};

use agent::Role;
use client::Message;
use model::GetStatus;
use state::App;

#[derive(Parser)]
#[command(
    name = "preparer",
    about = "OI 模拟赛组题助手：supervisor + 多个子 Agent（searching/statement/solution/auxiliary），\
支持工具调用、RAG 知识库、Skills。"
)]
struct Cli {
    /// OpenAI 兼容 base URL（不含 /chat/completions）。
    #[arg(long, short = 'u', env = "OPENAI_BASE_URL", default_value = "https://api.openai.com/v1", hide_env_values = true, global = true)]
    base_url: String,

    /// 模型供应商 API key。
    #[arg(long, short = 'k', env = "OPENAI_API_KEY", hide_env_values = true, global = true)]
    api_key: Option<String>,

    /// 模型名称。
    #[arg(long, short = 'm', default_value = "deepseek-v4-flash", global = true)]
    model: String,

    /// 每回合最大工具使用轮数。
    #[arg(long, default_value_t = 40, global = true)]
    max_steps: usize,

    /// embedding 模型（OpenAI 兼容 /embeddings）。省略则使用内置离线哈希 embedding。
    #[arg(long, global = true)]
    embedding_model: Option<String>,

    /// 指定比赛目录（相对或绝对路径）。
    #[arg(long, short = 'c', global = true)]
    contest: Option<String>,

    /// 查重后端：cpret（默认）或 yuantiji。
    #[arg(long, default_value = "cpret", global = true)]
    dup_backend: String,

    /// 首个任务。使用 "-" 从 stdin 读取所有行为任务。
    /// 交互模式下（有终端）后续回合从终端读取。
    prompt: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// 管理知识库（RAG 文档）。
    Kb {
        #[command(subcommand)]
        cmd: KbCmd,
    },
    /// 管理 skills。
    Skill {
        #[command(subcommand)]
        cmd: SkillCmd,
    },
    /// 管理会话（supervisor 对话历史）。
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// 导出为各种 OJ/评测工具格式。
    Export {
        #[command(subcommand)]
        cmd: ExportCmd,
    },
    /// 集成测试：编译、造数据、验证、运行 std 和 sols。
    Test {
        /// 指定题目 id（不指定则测试全部）。
        problem: Option<String>,
    },
    /// 打印当前工程状态（比赛/题目/组件）。
    Status,
}

#[derive(Subcommand)]
enum KbCmd {
    /// 添加纯文本文档到知识库。
    Add {
        path: String,
        /// 添加到全局知识库（~/.oiph/kb），默认添加到工程知识库（工程目录下 .oiph/kb）。
        #[arg(short = 'g', long)]
        global: bool,
    },
    /// 列出知识库内容（全局与工程）。
    List,
    /// 清空知识库（默认清空工程知识库，不存在则清空全局）。
    Clear {
        /// 清空全局知识库。
        #[arg(short = 'g', long)]
        global: bool,
    },
    /// 检索知识库（全局与工程合并）。
    Search {
        query: String,
        /// 返回数量（1-8，默认 4）。
        #[arg(long)]
        k: Option<u64>,
    },
}

#[derive(Subcommand)]
enum SkillCmd {
    /// 列出可用 skills（全局 + 工程）。
    List,
    /// 打印某 skill 的完整内容。
    Show { name: String },
    /// 从文件安装一个 skill（复制为 <名>/SKILL.md）。
    Add {
        path: String,
        /// skill 名称（默认取文件名去扩展名）。
        name: Option<String>,
        /// 安装到全局 skills（~/.oiph/skills），默认安装到工程 skills。
        #[arg(short = 'g', long)]
        global: bool,
    },
    /// 删除一个 skill。
    Delete {
        name: String,
        /// 删除全局 skills 中的，默认工程。
        #[arg(short = 'g', long)]
        global: bool,
    },
}

#[derive(Subcommand)]
enum SessionCmd {
    /// 列出当前比赛的会话。
    List,
    /// 新建会话并切换为当前。
    New { name: Option<String> },
    /// 切换到已有会话。
    Use { name: String },
    /// 删除会话。
    Delete { name: String },
    /// 导出会话为 markdown 文件。
    Export {
        /// 会话名（默认当前）。
        name: Option<String>,
        /// 输出路径（默认 <名>.md）。
        out: Option<String>,
    },
    /// 打印会话内容（markdown 到 stdout）。
    Show {
        /// 会话名（默认当前）。
        name: Option<String>,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WorkspaceState {
    #[serde(default)]
    current_contest: Option<String>,
}

const WS_STATE_FILE: &str = ".preparer.yaml";

fn load_ws_state(root: &Path) -> WorkspaceState {
    let p = root.join(WS_STATE_FILE);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_yaml::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_ws_state(root: &Path, ws: &WorkspaceState) -> Result<()> {
    std::fs::write(
        root.join(WS_STATE_FILE),
        serde_yaml::to_string(ws)?,
    )?;
    Ok(())
}

/// 解析当前比赛目录：
/// 1. 命令行 --contest；
/// 2. 工作目录本身是比赛工程；
/// 3. .preparer.yaml 记录的上次比赛。
fn resolve_contest(root: &Path, cli_contest: Option<&str>) -> Option<PathBuf> {
    if let Some(c) = cli_contest {
        let p = Path::new(c);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        };
        if project::is_contest_dir(&p) {
            return Some(p);
        }
        return None;
    }
    if project::is_contest_dir(root) {
        return Some(root.to_path_buf());
    }
    let ws = load_ws_state(root);
    ws.current_contest
        .as_deref()
        .map(|name| root.join(name))
        .filter(|p| project::is_contest_dir(p))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = std::env::current_dir()?;

    match &cli.command {
        Some(Commands::Kb { cmd }) => return run_kb_cmd(&cli, cmd).await,
        Some(Commands::Skill { cmd }) => return run_skill_cmd(&cli, cmd),
        Some(Commands::Session { cmd }) => return run_session_cmd(&cli, cmd),
        Some(Commands::Export { cmd }) => return run_export_cmd(&cli, cmd),
        Some(Commands::Test { problem }) => return run_test_cmd(&cli, problem.as_deref()),
        Some(Commands::Status) => {
            let contest_dir = resolve_contest(&root, cli.contest.as_deref());
            match contest_dir {
                Some(d) => print!("{}", project::status_text(&d)),
                None => println!("当前没有比赛工程（用 /contest new 或 create_contest 创建）"),
            }
            return Ok(());
        }
        None => {}
    }

    run_repl(&cli, &root).await
}

/// kb add/clear 的目标目录：指定 --global 用全局；否则用工程知识库（无比赛工程则全局）。
fn kb_target_dir(root: &Path, cli_contest: Option<&str>, global: bool) -> PathBuf {
    if global {
        return paths::global_kb_dir();
    }
    match resolve_contest(root, cli_contest) {
        Some(c) => paths::project_kb_dir(&c),
        None => paths::global_kb_dir(),
    }
}

async fn run_kb_cmd(cli: &Cli, cmd: &KbCmd) -> Result<()> {
    let root = std::env::current_dir()?;
    match cmd {
        KbCmd::Add { path, global } => {
            require_api_key_for_embeddings(cli.embedding_model.as_deref(), cli.api_key.as_deref())?;
            let dir = kb_target_dir(&root, cli.contest.as_deref(), *global);
            kb::cmd_add(
                path,
                &dir,
                &cli.base_url,
                cli.api_key.as_deref().unwrap_or(""),
                cli.embedding_model.as_deref(),
            )
            .await
        }
        KbCmd::List => {
            let mut dirs = vec![paths::global_kb_dir()];
            if let Some(c) = resolve_contest(&root, cli.contest.as_deref()) {
                dirs.push(paths::project_kb_dir(&c));
            }
            kb::cmd_list(&dirs)
        }
        KbCmd::Clear { global } => {
            let dir = kb_target_dir(&root, cli.contest.as_deref(), *global);
            kb::cmd_clear(&dir)
        }
        KbCmd::Search { query, k } => {
            let mut dirs = vec![paths::global_kb_dir()];
            if let Some(c) = resolve_contest(&root, cli.contest.as_deref()) {
                dirs.push(paths::project_kb_dir(&c));
            }
            let cfg = kb::KbConfig {
                dirs,
                base_url: cli.base_url.clone(),
                api_key: cli.api_key.clone().unwrap_or_default(),
                embed_model: cli.embedding_model.clone(),
            };
            let out = kb::search(&cfg, query, k.unwrap_or(4) as usize).await?;
            println!("{out}");
            Ok(())
        }
    }
}

/// skill add/delete 的目标目录：-g 用全局；否则工程（无比赛工程则全局）。
fn skill_target_dir(root: &Path, cli_contest: Option<&str>, global: bool) -> PathBuf {
    if global {
        return paths::global_skills_dir();
    }
    match resolve_contest(root, cli_contest) {
        Some(c) => paths::project_skills_dir(&c),
        None => paths::global_skills_dir(),
    }
}

fn run_skill_cmd(cli: &Cli, cmd: &SkillCmd) -> Result<()> {
    let root = std::env::current_dir()?;
    match cmd {
        SkillCmd::List => {
            let mut roots = vec![paths::global_skills_dir()];
            if let Some(c) = resolve_contest(&root, cli.contest.as_deref()) {
                roots.push(paths::project_skills_dir(&c));
            }
            let found = skills::discover(&roots);
            if found.is_empty() {
                println!("没有可用 skills");
            } else {
                for s in &found {
                    println!("- {}: {}", s.name, s.description);
                }
            }
            Ok(())
        }
        SkillCmd::Show { name } => {
            let mut roots = vec![paths::global_skills_dir()];
            if let Some(c) = resolve_contest(&root, cli.contest.as_deref()) {
                roots.push(paths::project_skills_dir(&c));
            }
            let content = skills::load_content(&roots, name)?;
            println!("{content}");
            Ok(())
        }
        SkillCmd::Add { path, name, global } => {
            let dir = skill_target_dir(&root, cli.contest.as_deref(), *global);
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("读取文件 '{path}' 失败"))?;
            let name = name.clone().unwrap_or_else(|| {
                Path::new(path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "skill".into())
            });
            let target = dir.join(&name).join(skills::SKILL_FILE);
            std::fs::create_dir_all(target.parent().expect("有父目录"))?;
            std::fs::write(&target, &content)?;
            println!("已安装 skill '{name}' 到 {}", dir.display());
            Ok(())
        }
        SkillCmd::Delete { name, global } => {
            let dir = skill_target_dir(&root, cli.contest.as_deref(), *global);
            let target = dir.join(name);
            if !target.exists() {
                bail!("skill '{name}' 不存在于 {}", dir.display());
            }
            std::fs::remove_dir_all(&target)?;
            println!("已删除 skill '{name}'");
            Ok(())
        }
    }
}

fn run_session_cmd(cli: &Cli, cmd: &SessionCmd) -> Result<()> {
    let root = std::env::current_dir()?;
    let Some(cdir) = resolve_contest(&root, cli.contest.as_deref()) else {
        bail!("当前没有比赛工程（session 存储于 <比赛工程>/.oiph/sessions/）");
    };
    match cmd {
        SessionCmd::List => {
            let metas = session::list(&cdir)?;
            if metas.is_empty() {
                println!("没有会话（用 `preparer session new` 新建）");
            }
            for m in &metas {
                println!(
                    "{} {} {}（{} 条消息）",
                    if m.current { "*" } else { " " },
                    m.name,
                    m.updated_at.format("%Y-%m-%d %H:%M:%S"),
                    m.messages
                );
            }
            Ok(())
        }
        SessionCmd::New { name } => {
            let name = match name {
                Some(n) => session::sanitize_name(n)?,
                None => session::auto_name(&cdir)?,
            };
            if session::exists(&cdir, &name) {
                bail!("会话 '{name}' 已存在");
            }
            let s = session::Session {
                name: name.clone(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                messages: Vec::new(),
            };
            session::save(&cdir, &s)?;
            println!("已新建并切换到会话 '{name}'");
            Ok(())
        }
        SessionCmd::Use { name } => {
            if !session::exists(&cdir, name) {
                bail!("会话 '{name}' 不存在");
            }
            session::set_current(&cdir, name)?;
            println!("已切换到会话 '{name}'");
            Ok(())
        }
        SessionCmd::Delete { name } => {
            session::delete(&cdir, name)?;
            if session::current_name(&cdir).as_deref() == Some(name.as_str()) {
                session::clear_current(&cdir);
            }
            println!("已删除会话 '{name}'");
            Ok(())
        }
        SessionCmd::Export { name, out } => {
            let name = resolve_session_name(&cdir, name.as_deref())?;
            let s = session::load(&cdir, &name)?;
            let out_path = out.clone().unwrap_or_else(|| format!("{name}.md"));
            std::fs::write(&out_path, session::export_markdown(&s))?;
            println!("已导出会话 '{name}' 到 {out_path}");
            Ok(())
        }
        SessionCmd::Show { name } => {
            let name = resolve_session_name(&cdir, name.as_deref())?;
            let s = session::load(&cdir, &name)?;
            print!("{}", session::export_markdown(&s));
            Ok(())
        }
    }
}

/// 解析 session 名：指定则校验存在；否则用 current，再否则报错。
fn resolve_session_name(cdir: &Path, name: Option<&str>) -> Result<String> {
    match name {
        Some(n) => {
            if !session::exists(cdir, n) {
                bail!("会话 '{n}' 不存在");
            }
            Ok(n.to_string())
        }
        None => session::current_name(cdir)
            .ok_or_else(|| anyhow!("没有当前会话，请指定名称或用 `preparer session new`")),
    }
}

#[derive(Subcommand)]
enum ExportCmd {
    /// 导出为 LemonLime 格式。
    Lemon {
        /// 输出目录（默认 <比赛目录>/<比赛名>_lemon/）。
        output: Option<String>,
    },
}

fn run_export_cmd(cli: &Cli, cmd: &ExportCmd) -> Result<()> {
    let root = std::env::current_dir()?;
    let Some(cdir) = resolve_contest(&root, cli.contest.as_deref()) else {
        bail!("当前没有比赛工程（export 需要比赛目录）");
    };
    match cmd {
        ExportCmd::Lemon { output } => {
            let out = output.as_deref().map(Path::new);
            let path = export_lemon::export(&cdir, out)?;
            println!("已导出到 {}", path.display());
            Ok(())
        }
    }
}

fn run_test_cmd(cli: &Cli, problem: Option<&str>) -> Result<()> {
    let root = std::env::current_dir()?;
    let Some(cdir) = resolve_contest(&root, cli.contest.as_deref()) else {
        bail!("当前没有比赛工程（test 需要比赛目录）");
    };
    let reports = test_runner::run_tests(&cdir, problem);
    let mut has_error = false;
    for report in &reports {
        let s = report.to_string_report();
        if report.errors.is_empty() {
            println!("{s}");
        } else {
            eprint!("{s}");
            has_error = true;
        }
    }
    if has_error {
        bail!("集成测试存在错误");
    }
    Ok(())
}

fn require_api_key_for_embeddings(embed_model: Option<&str>, api_key: Option<&str>) -> Result<()> {
    if embed_model.is_some() && api_key.is_none() {
        bail!("--embedding-model 需要 API key（--api-key 或 OPENAI_API_KEY）");
    }
    Ok(())
}

/// 启动时把内置知识库文档种子到全局知识库（~/.oiph/kb），内置 skills 种子到
/// 全局 skills（~/.oiph/skills）。已存在的跳过。
async fn seed_builtin(app: &App) {
    // 知识库：provider embedding 且无 key 时跳过（本地哈希 embedding 无需 key）
    if app.embed_model.is_none() || !app.api_key.is_empty() {
        match kb::ensure_builtin(
            &paths::global_kb_dir(),
            assets::KB_DOCS,
            &app.base_url,
            &app.api_key,
            app.embed_model.as_deref(),
        )
        .await
        {
            Ok(0) => {}
            Ok(n) => println!(
                "已将 {} 篇内置知识库文档种子到 {}",
                n,
                paths::global_kb_dir().display()
            ),
            Err(e) => eprintln!("内置知识库种子失败：{e:#}"),
        }
    }
    match skills::ensure_builtin(&paths::global_skills_dir(), assets::BUILTIN_SKILLS) {
        Ok(0) => {}
        Ok(n) => println!(
            "已写入 {n} 个内置 skill 到 {}",
            paths::global_skills_dir().display()
        ),
        Err(e) => eprintln!("内置 skills 种子失败：{e:#}"),
    }
}

async fn run_repl(cli: &Cli, root: &Path) -> Result<()> {
    let dup_backend = dupcheck::Backend::parse(&cli.dup_backend)
        .unwrap_or(dupcheck::Backend::Cpret);
    let app = Arc::new(App::new(
        root.to_path_buf(),
        cli.base_url.clone(),
        cli.api_key.clone().unwrap_or_default(),
        cli.embedding_model.clone(),
        cli.model.clone(),
        cli.max_steps,
        dup_backend,
    )?);

    let contest_dir = resolve_contest(root, cli.contest.as_deref());
    app.set_contest_dir(contest_dir.clone());

    seed_builtin(&app).await;

    let tty = io::stdin().is_terminal();

    // 在比赛工程下自动加载上一次的 session
    let mut current_session: Option<String> = None;
    let mut messages: Vec<Message> = match &contest_dir {
        Some(c) => match session::last(c) {
            Ok(Some(s)) => {
                let name = s.name.clone();
                let mut msgs = s.messages;
                if msgs.is_empty() || msgs[0].role != "system" {
                    msgs.insert(0, agent::system_message_for(Role::Supervisor, &app));
                }
                current_session = Some(name);
                if tty {
                    println!("已加载会话：{}", current_session.as_deref().unwrap_or(""));
                }
                msgs
            }
            Ok(None) => vec![agent::system_message_for(Role::Supervisor, &app)],
            Err(e) => {
                eprintln!("加载会话失败：{e:#}");
                vec![agent::system_message_for(Role::Supervisor, &app)]
            }
        },
        None => vec![agent::system_message_for(Role::Supervisor, &app)],
    };

    if tty {
        println!(
            "OI 模拟赛组题助手（preparer）。当前比赛：{}",
            contest_dir
                .as_deref()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "（无）".into())
        );
        println!("输入任务描述开始，输入 /help 查看指令，/exit 退出。");
    }

    let mut pending: VecDeque<String> = Default::default();
    match cli.prompt.as_deref() {
        Some("-") => pending.extend(read_all_stdin()?.lines().map(|l| l.trim().to_string())),
        Some(p) => {
            pending.push_back(p.trim().to_string());
            if !tty {
                pending.extend(read_all_stdin()?.lines().map(|l| l.trim().to_string()));
            }
        }
        None => {
            if !tty {
                pending.extend(read_all_stdin()?.lines().map(|l| l.trim().to_string()));
            }
        }
    }

    let mut rl = new_editor();

    loop {
        let line = match pending.pop_front() {
            Some(t) => t,
            None if tty => match repl_readline(&mut rl)? {
                Some(l) => l,
                None => break,
            },
            None => break,
        };
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('/') {
            match handle_slash(&app, root, &trimmed, &mut messages, &mut current_session).await {
                Ok(SlashOutcome::Exit) => break,
                Ok(SlashOutcome::Continue) => {}
                Err(e) => eprintln!("错误：{e:#}"),
            }
            continue;
        }

        if app.api_key.is_empty() {
            eprintln!("需要 API key：--api-key 或 OPENAI_API_KEY（查看状态可用 /status）");
            if !tty {
                return Err(anyhow!("缺少 API key"));
            }
            continue;
        }

        // 刷新系统消息（skills 可能中途变化）
        messages[0] = agent::system_message_for(Role::Supervisor, &app);
        messages.push(Message::user(trimmed.clone()));
        let deps = app.deps();

        // 启用 raw 模式 + Esc 监视
        let cancel = term::CancelFlag::new();
        let raw_ok = tty && crossterm::terminal::enable_raw_mode().is_ok();
        term::set_raw(raw_ok);
        let watcher = if raw_ok {
            Some(term::EscWatcher::start(cancel.clone()))
        } else {
            None
        };

        let result = agent::run_turn(
            &deps,
            &app,
            Role::Supervisor,
            None,
            &mut messages,
            true,  // supervisor 实时打印流式内容
            true,  // supervisor 显示思维链
            &cancel,
        )
        .await;

        // 恢复终端
        drop(watcher);
        if raw_ok {
            let _ = crossterm::terminal::disable_raw_mode();
        }
        term::set_raw(false);

        match result {
            Ok(turn_result) => {
                if turn_result.interrupted {
                    term::println_err("\n⚠ 已打断（保留已有输出）");
                } else if turn_result.text.trim().is_empty() && !messages.is_empty() {
                    // 模型可能只调用了工具，没有最终文本
                }
                // 打印换行分隔
                println!();
            }
            Err(e) => {
                term::println_err(&format!("\n⚠ LLM 调用失败（已重试 5 次）：{e:#}"));
                term::println_err("请检查网络连接或 API 配置后重新输入。");
            }
        }
        // 保存会话
        save_current_session(&app, &messages, &mut current_session);
    }

    // 保存历史
    let _ = rl.save_history(&dirs_for_history());
    Ok(())
}
fn save_current_session(app: &App, messages: &[Message], current_session: &mut Option<String>) {
    let Some(cdir) = app.contest_dir() else {
        return;
    };
    let name = match current_session.clone() {
        Some(n) => n,
        None => match session::auto_name(&cdir) {
            Ok(n) => {
                *current_session = Some(n.clone());
                n
            }
            Err(e) => {
                eprintln!("生成会话名失败：{e:#}");
                return;
            }
        },
    };
    if let Err(e) = session::save_messages(&cdir, &name, messages) {
        eprintln!("保存会话失败：{e:#}");
    }
}

enum SlashOutcome {
    Continue,
    Exit,
}

async fn handle_slash(
    app: &App,
    root: &Path,
    input: &str,
    messages: &mut Vec<Message>,
    current_session: &mut Option<String>,
) -> Result<SlashOutcome> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let (cmd, args) = parts.split_first().expect("non-empty");
    let cmd = cmd.trim_start_matches('/');
    match cmd {
        "help" => {
            print_help();
            Ok(SlashOutcome::Continue)
        }
        "exit" | "quit" => Ok(SlashOutcome::Exit),
        "status" => {
            let target = match args.first() {
                Some(pid) => app.contest_dir().map(|d| {
                    let pdir = project::problem_dir(&d, pid);
                    (d, pdir)
                }),
                None => None,
            };
            match app.contest_dir() {
                None => println!("当前没有比赛工程（/contest new <名称> 创建）"),
                Some(d) => {
                    if let Some((_, pdir)) = target {
                        if pdir.exists() {
                            match project::load_problem(&pdir) {
                                Ok(p) => print!("{}", project::problem_status_text(&p, &d)),
                                Err(e) => println!("读取题目失败：{e:#}"),
                            }
                        } else {
                            println!("题目目录不存在：{}", pdir.display());
                        }
                    } else {
                        print!("{}", project::status_text(&d));
                    }
                }
            }
            Ok(SlashOutcome::Continue)
        }
        "contest" => {
            handle_slash_contest(app, root, args).await;
            Ok(SlashOutcome::Continue)
        }
        "problem" => {
            handle_slash_problem(app, args).await;
            Ok(SlashOutcome::Continue)
        }
        "kb" => {
            handle_slash_kb(app, args).await?;
            Ok(SlashOutcome::Continue)
        }
        "skill" => {
            handle_slash_skill(app, args).await?;
            Ok(SlashOutcome::Continue)
        }
        "session" => {
            handle_slash_session(app, args, messages, current_session).await?;
            Ok(SlashOutcome::Continue)
        }
        other => {
            eprintln!("未知指令 /{other}，输入 /help 查看可用指令");
            Ok(SlashOutcome::Continue)
        }
    }
}

fn print_help() {
    println!(
        "\
本地指令（以 / 开头，不经 LLM）：
  /help               显示帮助
  /status [题目id]    查看比赛整体或单个题目各组件状态
  /contest list       列出工作目录下的比赛
  /contest new <名>   新建比赛并切换
  /contest use <目录> 切换当前比赛
  /problem list       列出当前比赛题目
  /problem add <id> [类型]  添加题目
  /problem show <id>  查看题目配置与文件
  /kb add <文件> [global]    添加知识库文档（默认工程，无比赛则全局）
  /kb list            列出知识库（全局与工程）
  /kb clear [global]  清空知识库
  /skill list         列出可用 skills
  /skill show <名>    查看 skill 内容
  /session list       列出会话
  /session new [名]   新建会话并切换
  /session use <名>   切换会话
  /session delete <名> 删除会话
  /session export [名] [路径]  导出会话为 markdown
  /exit               退出

其余输入将交给 supervisor agent（对话自动保存到 <比赛工程>/.oiph/sessions/）。
环境变量：OPENAI_BASE_URL、OPENAI_API_KEY。"
    );
}

async fn handle_slash_contest(app: &App, root: &Path, args: &[&str]) {
    match args {
        [] => match app.contest_dir() {
            Some(d) => println!("当前比赛：{}", d.display()),
            None => println!("当前没有比赛（/contest new <名> 创建，/contest use <目录> 切换）"),
        },
        ["list"] => {
            let mut found = Vec::new();
            if let Ok(rd) = std::fs::read_dir(root) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() && project::is_contest_dir(&p) {
                        found.push(p.file_name().unwrap_or_default().to_string_lossy().into_owned());
                    }
                }
            }
            if found.is_empty() {
                println!("工作目录下没有比赛工程");
            } else {
                for f in found {
                    let mark = app
                        .contest_dir()
                        .as_deref()
                        .map(|d| d.file_name() == Some(std::ffi::OsStr::new(&f)))
                        .unwrap_or(false);
                    println!("{} {f}", if mark { "*" } else { " " });
                }
            }
        }
        ["new", name] => {
            if project::is_contest_dir(root) {
                eprintln!("错误：当前目录已是比赛工程目录");
                return;
            }
            match project::init_contest(root, name) {
                Ok(_) => {
                    app.set_contest_dir(Some(root.to_path_buf()));
                    println!("已在当前目录创建比赛 {name}：{}", root.display());
                }
                Err(e) => eprintln!("错误：{e:#}"),
            }
        }
        ["use", dir] => {
            let p = Path::new(dir);
            let p = if p.is_absolute() {
                p.to_path_buf()
            } else {
                root.join(p)
            };
            if project::is_contest_dir(&p) {
                app.set_contest_dir(Some(p.clone()));
                let ws = WorkspaceState {
                    current_contest: p
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned()),
                };
                let _ = save_ws_state(root, &ws);
                println!("已切换到比赛：{}", p.display());
            } else {
                eprintln!("不是比赛工程目录：{}", p.display());
            }
        }
        _ => eprintln!("用法：/contest list | /contest new <名> | /contest use <目录>"),
    }
}

async fn handle_slash_problem(app: &App, args: &[&str]) {
    let Some(contest_dir) = app.contest_dir() else {
        eprintln!("当前没有比赛（/contest new <名> 创建）");
        return;
    };
    match args {
        [] | ["list"] => {
            match project::load_contest(&contest_dir) {
                Ok(c) => {
                    if c.problems.is_empty() {
                        println!("（无题目，用 /problem add <id> 添加）");
                    }
                    for pid in &c.problems {
                        if let Ok(p) = project::load_problem(&project::problem_dir(&contest_dir, pid)) {
                            println!(
                                "- {pid}（{}，{}）{}",
                                p.name,
                                p.problem_type.label(),
                                p.get_status().label()
                            );
                        } else {
                            println!("- {pid}（配置读取失败）");
                        }
                    }
                }
                Err(e) => eprintln!("错误：{e:#}"),
            }
        }
        ["add", id, rest @ ..] => match project::add_problem(
            &contest_dir,
            project::NewProblem {
                id,
                name: None,
                problem_type: rest.first().and_then(|s| parse_type_arg(s)),
                source: None,
            },
        ) {
            Ok(_) => println!("已添加题目 {id}"),
            Err(e) => eprintln!("错误：{e:#}"),
        },
        ["show", id] => {
            let pdir = project::problem_dir(&contest_dir, id);
            match project::load_problem(&pdir) {
                Ok(p) => {
                    println!("{}", serde_yaml::to_string(&p).unwrap_or_default());
                    print!("{}", project::problem_files_listing(&pdir));
                }
                Err(e) => eprintln!("错误：{e:#}"),
            }
        }
        _ => eprintln!("用法：/problem list | /problem add <id> [类型] | /problem show <id>"),
    }
}

fn parse_type_arg(s: &str) -> Option<model::ProblemType> {
    let t = s.to_lowercase();
    Some(match t.as_str() {
        "traditional" | "传统" => model::ProblemType::Traditional,
        "interactive_lib" | "函数交互" => model::ProblemType::InteractiveLib,
        "interactive_io" | "io交互" => model::ProblemType::InteractiveIO,
        "answer_only" | "提交答案" => model::ProblemType::AnswerOnly,
        "function" | "函数题" => model::ProblemType::Function,
        _ => return None,
    })
}

async fn handle_slash_kb(app: &App, args: &[&str]) -> Result<()> {
    let dirs = app.kb_dirs();
    match args {
        ["add", path] | ["add", path, "global"] => {
            if app.embed_model.is_some() && app.api_key.is_empty() {
                bail!("--embedding-model 需要 API key");
            }
            let global = args.get(2).is_some_and(|s| *s == "global");
            let dir = if global {
                dirs[0].clone()
            } else if app.contest_dir().is_some() {
                dirs[1].clone()
            } else {
                dirs[0].clone()
            };
            kb::cmd_add(
                path,
                &dir,
                &app.base_url,
                &app.api_key,
                app.embed_model.as_deref(),
            )
            .await
        }
        ["list"] => kb::cmd_list(&dirs),
        ["clear"] | ["clear", "global"] => {
            let global = args.get(1).is_some_and(|s| *s == "global");
            let dir = if global {
                dirs[0].clone()
            } else if dirs[1].exists() {
                dirs[1].clone()
            } else {
                dirs[0].clone()
            };
            kb::cmd_clear(&dir)
        }
        _ => {
            println!("用法：/kb add <文件> [global] | /kb list | /kb clear [global]");
            Ok(())
        }
    }
}

async fn handle_slash_skill(app: &App, args: &[&str]) -> Result<()> {
    let roots = app.skill_roots();
    match args {
        [] | ["list"] => {
            let found = skills::discover(&roots);
            if found.is_empty() {
                println!("没有可用 skills");
            } else {
                for s in &found {
                    println!("- {}: {}", s.name, s.description);
                }
            }
        }
        ["show", name] => {
            let content = skills::load_content(&roots, name)?;
            println!("{content}");
        }
        _ => println!("用法：/skill list | /skill show <名>"),
    }
    Ok(())
}

async fn handle_slash_session(
    app: &App,
    args: &[&str],
    messages: &mut Vec<Message>,
    current_session: &mut Option<String>,
) -> Result<()> {
    let Some(cdir) = app.contest_dir() else {
        bail!("session 需要比赛工程（/contest new <名> 创建）");
    };
    match args {
        [] | ["list"] => {
            let metas = session::list(&cdir)?;
            if metas.is_empty() {
                println!("没有会话（/session new [名] 新建）");
            }
            for m in &metas {
                println!(
                    "{} {} {}（{} 条消息）",
                    if m.current { "*" } else { " " },
                    m.name,
                    m.updated_at.format("%Y-%m-%d %H:%M:%S"),
                    m.messages
                );
            }
        }
        ["new"] | ["new", _] => {
            // 先保存当前
            save_current_session(app, messages, current_session);
            let name = match args.get(1) {
                Some(n) => session::sanitize_name(n)?,
                None => session::auto_name(&cdir)?,
            };
            if session::exists(&cdir, &name) {
                bail!("会话 '{name}' 已存在");
            }
            *messages = vec![agent::system_message_for(Role::Supervisor, app)];
            let s = session::Session {
                name: name.clone(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                messages: messages.clone(),
            };
            session::save(&cdir, &s)?;
            *current_session = Some(name.clone());
            println!("已新建并切换到会话 '{name}'");
        }
        ["use", name] => {
            if !session::exists(&cdir, name) {
                bail!("会话 '{name}' 不存在");
            }
            save_current_session(app, messages, current_session);
            let s = session::load(&cdir, name)?;
            *messages = s.messages.clone();
            if messages.is_empty() || messages[0].role != "system" {
                messages.insert(0, agent::system_message_for(Role::Supervisor, app));
            }
            session::set_current(&cdir, name)?;
            *current_session = Some(name.to_string());
            println!("已切换到会话 '{name}'（{} 条消息）", s.messages.len());
        }
        ["delete", name] => {
            if current_session.as_deref() == Some(name) {
                bail!("不能删除当前会话，请先 /session use 切换到其他会话");
            }
            session::delete(&cdir, name)?;
            println!("已删除会话 '{name}'");
        }
        ["export"] | ["export", _] | ["export", _, _] => {
            let name = match args.get(1) {
                Some(n) if session::exists(&cdir, n) => n.to_string(),
                _ => current_session
                    .clone()
                    .ok_or_else(|| anyhow!("没有当前会话，请指定名称"))?,
            };
            let s = session::load(&cdir, &name)?;
            let out = args
                .get(2)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{name}.md"));
            std::fs::write(&out, session::export_markdown(&s))?;
            println!("已导出会话 '{name}' 到 {out}");
        }
        _ => println!(
            "用法：/session list | /session new [名] | /session use <名> | /session delete <名> | /session export [名] [路径]"
        ),
    }
    Ok(())
}

fn read_all_stdin() -> Result<String> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

/// rustyline 编辑器（多字节安全、历史记录）。
fn new_editor() -> Editor<(), rustyline::history::FileHistory> {
    let mut rl: Editor<(), rustyline::history::FileHistory> = Editor::new().unwrap_or_else(|_| {
        let config = rustyline::Config::builder().build();
        rustyline::Editor::with_config(config).unwrap()
    });
    let hist = dirs_for_history();
    let _ = rl.load_history(&hist);
    rl
}

fn dirs_for_history() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    home.join(".oiph").join("history.txt")
}

fn repl_readline(rl: &mut Editor<(), rustyline::history::FileHistory>) -> Result<Option<String>> {
    match rl.readline(">> ") {
        Ok(line) => {
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                rl.add_history_entry(&line).ok();
            }
            Ok(Some(trimmed))
        }
        Err(rustyline::error::ReadlineError::Eof) => Ok(None),
        Err(rustyline::error::ReadlineError::Interrupted) => Ok(Some(String::new())),
        Err(e) => Err(anyhow!("读取输入失败：{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_type_arg_works() {
        assert_eq!(parse_type_arg("traditional"), Some(model::ProblemType::Traditional));
        assert_eq!(parse_type_arg("函数交互"), Some(model::ProblemType::InteractiveLib));
        assert_eq!(parse_type_arg("nope"), None);
    }

    #[test]
    fn ws_state_roundtrip() {
        let d = std::env::temp_dir().join(format!("preparer_test_ws_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let ws = WorkspaceState {
            current_contest: Some("abc".into()),
        };
        save_ws_state(&d, &ws).unwrap();
        let loaded = load_ws_state(&d);
        assert_eq!(loaded.current_contest.as_deref(), Some("abc"));
        std::fs::remove_dir_all(&d).ok();
    }
}
