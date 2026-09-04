//! 工程持久化：比赛/题目的 `config.yaml` 读写、目录结构管理、状态更新。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::model::{
    Component, ComponentStatus, Contest, DuplicateCheckResult, GetStatus, Problem,
    ProblemSource, ProblemType, SolutionStatus,
};

pub const CONFIG_NAME: &str = "config.yaml";

// ---------------------------------------------------------------------------
// 路径
// ---------------------------------------------------------------------------

pub fn contest_config_path(contest_dir: &Path) -> PathBuf {
    contest_dir.join(CONFIG_NAME)
}

pub fn problem_dir(contest_dir: &Path, id: &str) -> PathBuf {
    contest_dir.join(id)
}

pub fn is_contest_dir(dir: &Path) -> bool {
    contest_config_path(dir).is_file()
}

// ---------------------------------------------------------------------------
// 比赛
// ---------------------------------------------------------------------------

pub fn init_contest(dir: &Path, name: &str) -> Result<Contest> {
    let cfg = contest_config_path(dir);
    if cfg.exists() {
        bail!("目录 {} 已存在比赛工程配置", dir.display());
    }
    std::fs::create_dir_all(dir)
        .with_context(|| format!("无法创建比赛目录 {}", dir.display()))?;
    let contest = Contest::new(name);
    save_contest(dir, &contest)?;
    Ok(contest)
}

/// 加载比赛元信息（不加载题目详情）。
fn load_contest_raw(dir: &Path) -> Result<Option<Contest>> {
    let path = contest_config_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读取比赛配置失败：{}", path.display()))?;
    let c: Contest = serde_yaml::from_str(&raw)
        .with_context(|| format!("解析比赛配置失败：{}", path.display()))?;
    Ok(Some(c))
}

/// 加载比赛并填充题目列表。
pub fn load_contest(dir: &Path) -> Result<Contest> {
    let mut c = load_contest_raw(dir)?
        .ok_or_else(|| anyhow!("目录 {} 下没有 {}，不是比赛工程", dir.display(), CONFIG_NAME))?;
    c.loaded_problems = c
        .problems
        .iter()
        .map(|pid| load_problem(&problem_dir(dir, pid)))
        .collect::<Result<Vec<_>>>()?;
    Ok(c)
}

pub fn save_contest(dir: &Path, contest: &Contest) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("无法创建比赛目录 {}", dir.display()))?;
    let yaml = serde_yaml::to_string(contest)
        .context("序列化比赛配置失败")?;
    let path = contest_config_path(dir);
    std::fs::write(&path, yaml)
        .with_context(|| format!("写入比赛配置失败：{}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 题目
// ---------------------------------------------------------------------------

pub fn load_problem(pdir: &Path) -> Result<Problem> {
    let path = pdir.join(CONFIG_NAME);
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读取题目配置失败：{}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("解析题目配置失败：{}", path.display()))
}

pub fn save_problem(pdir: &Path, p: &Problem) -> Result<()> {
    std::fs::create_dir_all(pdir)
        .with_context(|| format!("无法创建题目目录 {}", pdir.display()))?;
    let yaml = serde_yaml::to_string(p).context("序列化题目配置失败")?;
    let path = pdir.join(CONFIG_NAME);
    std::fs::write(&path, yaml)
        .with_context(|| format!("写入题目配置失败：{}", path.display()))?;
    Ok(())
}

pub struct NewProblem<'a> {
    pub id: &'a str,
    pub name: Option<&'a str>,
    pub problem_type: Option<ProblemType>,
    pub source: Option<ProblemSource>,
}

pub fn add_problem(contest_dir: &Path, req: NewProblem) -> Result<Problem> {
    sanitize_id(req.id)?;
    let pid = req.id;
    let pdir = problem_dir(contest_dir, pid);
    if pdir.exists() {
        bail!("题目目录 {} 已存在", pdir.display());
    }

    let mut problem = Problem::new(pid);
    problem.name = req.name.unwrap_or("").to_string();
    if let Some(t) = req.problem_type {
        problem.problem_type = t;
    }
    if let Some(s) = req.source {
        problem.source = s;
    }
    // 建目录结构
    for sub in [
        "statement",
        "statement/down",
        "data",
        "auxiliary",
        "solutions",
    ] {
        std::fs::create_dir_all(pdir.join(sub))
            .with_context(|| format!("创建 {} 失败", sub))?;
    }
    // subtasks 与 data_gen 在 problem config.yaml 中（subtasks 默认空列表）
    save_problem(&pdir, &problem)?;

    // 更新比赛 problem 列表
    let mut contest = load_contest_raw(contest_dir)?
        .ok_or_else(|| anyhow!("当前目录不是比赛工程"))?;
    if !contest.problems.iter().any(|p| p == pid) {
        contest.problems.push(pid.to_string());
        save_contest(contest_dir, &contest)?;
    }
    Ok(problem)
}

/// 根据 `want` 解析题目 id：指定则校验存在；未指定且只有一个题目则用它。
pub fn resolve_problem_id(contest_dir: &Path, want: Option<&str>) -> Result<String> {
    let contest = load_contest_raw(contest_dir)?
        .ok_or_else(|| anyhow!("当前目录不是比赛工程"))?;
    match want {
        Some(pid) => {
            if contest.problems.iter().any(|p| p == pid) {
                Ok(pid.to_string())
            } else {
                bail!("题目 {pid} 不存在于比赛 {}", contest.name);
            }
        }
        None => {
            if contest.problems.len() == 1 {
                Ok(contest.problems[0].clone())
            } else if contest.problems.is_empty() {
                Err(anyhow!("当前比赛没有题目"))
            } else {
                Err(anyhow!(
                    "比赛有多个题目，需指定 problem 参数；可选：{}",
                    contest.problems.join(", ")
                ))
            }
        }
    }
}

pub fn with_problem_mut<F, R>(contest_dir: &Path, pid: &str, f: F) -> Result<R>
where
    F: FnOnce(&mut Problem) -> Result<R>,
{
    let pdir = problem_dir(contest_dir, pid);
    let mut p = load_problem(&pdir)?;
    let r = f(&mut p)?;
    save_problem(&pdir, &p)?;
    Ok(r)
}

// ---------------------------------------------------------------------------
// 组件状态更新
// ---------------------------------------------------------------------------

/// 组件引用，按字符串解析。
/// 接受：statement / std / data / validator / checker / interactive_lib / tutorial /
/// sols（全部解法）/ sol:<name>（单个解法）。
pub fn set_component_status(
    contest_dir: &Path,
    pid: &str,
    component: &str,
    status: ComponentStatus,
) -> Result<Problem> {
    with_problem_mut(contest_dir, pid, |p| {
        match component {
            "statement" => p.statement = status.clone(),
            "std" => p.std.status = status.clone(),
            "data" => p.data.status = status.clone(),
            "validator" => p.validator.status = status.clone(),
            "checker" => p.checker.status = status.clone(),
            "tutorial" => p.tutorial = status.clone(),
            "interactive_lib" => {
                let c = p.interactive_lib.get_or_insert_with(Component::new);
                c.status = status.clone();
            }
            "sols" => {
                for s in &mut p.sols {
                    s.status = status.clone();
                }
            }
            other if other.starts_with("sol:") => {
                let name = other.trim_start_matches("sol:");
                let s = p
                    .sols
                    .iter_mut()
                    .find(|s| s.name == name)
                    .ok_or_else(|| anyhow!("未找到解法 {name}，请先用 add_solution 登记"))?;
                s.status = status.clone();
            }
            other => bail!("未知组件：{other}（可用：statement/std/data/validator/checker/interactive_lib/tutorial/sols/sol:<name>）"),
        }
        Ok(p.clone())
    })
}

pub fn set_duplicate_check(
    contest_dir: &Path,
    pid: &str,
    result: DuplicateCheckResult,
) -> Result<()> {
    with_problem_mut(contest_dir, pid, |p| {
        p.duplicate_check = Some(result.clone());
        Ok(())
    })
}

pub fn add_solution(contest_dir: &Path, pid: &str, sol: SolutionStatus) -> Result<()> {
    with_problem_mut(contest_dir, pid, |p| {
        if p.sols.iter().any(|s| s.name == sol.name) {
            bail!("解法 {} 已存在", sol.name);
        }
        p.sols.push(sol.clone());
        Ok(())
    })
}

/// 题目元信息更新参数。
#[derive(Default, Clone)]
pub struct ProblemMeta {
    pub name: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: Option<ProblemSource>,
    pub problem_type: Option<ProblemType>,
    pub time_limit_ms: Option<u64>,
    pub memory_limit_mb: Option<u64>,
}

pub fn set_problem_meta(contest_dir: &Path, pid: &str, meta: ProblemMeta) -> Result<()> {
    with_problem_mut(contest_dir, pid, |p| {
        if let Some(n) = meta.name {
            p.name = n;
        }
        if let Some(t) = meta.tags {
            p.tags = t;
        }
        if let Some(s) = meta.source {
            p.source = s;
        }
        if let Some(t) = meta.problem_type {
            p.problem_type = t;
        }
        if let Some(t) = meta.time_limit_ms {
            p.time_limit_ms = t;
        }
        if let Some(m) = meta.memory_limit_mb {
            p.memory_limit_mb = m;
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// 工具：状态展示
// ---------------------------------------------------------------------------

/// 返回题目目录下各子目录的文件清单文本（用于 `get_problem` 工具）。
pub fn problem_files_listing(pdir: &Path) -> String {
    let mut out = String::new();
    for (sub, _) in [
        ("statement", "题面"),
        ("data", "数据"),
        ("auxiliary", "辅助程序"),
        ("solutions", "解法"),
    ] {
        out.push_str(&format!("{sub}（{}）：\n", label_zh(sub)));
        let dir = pdir.join(sub);
        if !dir.exists() {
            out.push_str("  （目录不存在）\n");
            continue;
        }
        let mut entries: Vec<(PathBuf, u64)> = Vec::new();
        collect_dir(&dir, &mut entries, 0, 3);
        if entries.is_empty() {
            out.push_str("  （空）\n");
        } else {
            for (p, size) in entries {
                let rel = p
                    .strip_prefix(&dir)
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_default();
                out.push_str(&format!("  {rel}（{size} B）\n"));
            }
        }
    }
    out
}

fn label_zh(sub: &str) -> &'static str {
    match sub {
        "statement" => "题面",
        "data" => "数据",
        "auxiliary" => "辅助程序",
        "solutions" => "解法",
        _ => "",
    }
}

fn collect_dir(cur: &Path, out: &mut Vec<(PathBuf, u64)>, depth: usize, max_depth: usize) {
    let Ok(rd) = std::fs::read_dir(cur) else {
        return;
    };
    let mut items: Vec<_> = rd.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for e in items {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if depth + 1 < max_depth {
                collect_dir(&p, out, depth + 1, max_depth);
            } else if let Ok(meta) = e.metadata() {
                out.push((p, meta.len()));
            }
        } else if let Ok(meta) = e.metadata() {
            out.push((p, meta.len()));
        }
    }
}

/// 生成整个比赛的状态树文本（用于 `/status` 与 `get_project_status`）。
pub fn status_text(contest_dir: &Path) -> String {
    match load_contest(contest_dir) {
        Ok(c) => contest_status_text(&c, contest_dir),
        Err(e) => format!("加载比赛失败：{e:#}"),
    }
}

pub fn contest_status_text(c: &Contest, contest_dir: &Path) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "比赛 {}（id: {}）\n目录：{}\n整体状态：{}\n题目数：{}\n",
        c.name,
        c.id,
        contest_dir.display(),
        c.get_status().label(),
        c.loaded_problems.len(),
    ));
    if c.loaded_problems.is_empty() {
        out.push_str("（尚无题目）\n");
        return out;
    }
    for p in &c.loaded_problems {
        out.push_str(&problem_status_text(p, contest_dir));
        out.push('\n');
    }
    out
}

pub fn problem_status_text(p: &Problem, contest_dir: &Path) -> String {
    let pdir = problem_dir(contest_dir, &p.id);
    let mut out = String::new();
    out.push_str(&format!(
        "── 题目 {}（{}，{}，{}）  状态：{}\n",
        p.id,
        if p.name.is_empty() { "(未命名)" } else { &p.name },
        p.problem_type.label(),
        p.source.label(),
        p.get_status().label(),
    ));
    out.push_str(&format!("目录：{}\n", pdir.display()));
    if !p.tags.is_empty() {
        out.push_str(&format!("标签：{}\n", p.tags.join(", ")));
    }
    out.push_str(&format!("时限 {}ms  空间 {}MB\n", p.time_limit_ms, p.memory_limit_mb));
    out.push_str(&format!("statement：{}\n", p.statement.label()));
    out.push_str(&format!(
        "std：{}（预期 {} {}）\n",
        p.std.status.label(),
        p.std.expected.verdict.as_str(),
        p.std
            .expected
            .score
            .map(|s| format!("{s}"))
            .unwrap_or_else(|| "-".into()),
    ));
    if p.sols.is_empty() {
        out.push_str("sols：（无）\n");
    } else {
        out.push_str("sols：\n");
        for s in &p.sols {
            out.push_str(&format!(
                "  - {}：{}  预期 {} {}\n",
                s.name,
                s.status.label(),
                s.expected.verdict.as_str(),
                s.expected
                    .score
                    .map(|x| format!("{x}"))
                    .unwrap_or_else(|| "-".into()),
            ));
        }
    }
    out.push_str(&format!("data：{}\n", p.data.status.label()));
    out.push_str(&format!("validator：{}\n", p.validator.status.label()));
    out.push_str(&format!("checker：{}\n", p.checker.status.label()));
    if let Some(il) = &p.interactive_lib {
        out.push_str(&format!("interactive_lib：{}\n", il.status.label()));
    }
    out.push_str(&format!("tutorial：{}\n", p.tutorial.label()));
    out.push_str(&format!(
        "duplicate_check：{}\n",
        match &p.duplicate_check {
            None => "未查重".to_string(),
            Some(r) => {
                if r.found {
                    format!("发现疑似原题（{}）", r.matches.join("；"))
                } else {
                    "未发现原题".to_string()
                }
            }
        },
    ));
    out.push_str(&format!(
        "last_tested：{}\n",
        match &p.last_tested {
            None => "未测试".to_string(),
            Some(t) => {
                let label = format!("{}（已通过）", t.format("%Y-%m-%d %H:%M:%S"));
                if is_stale(contest_dir, &p.id, *t) {
                    format!("{}（可能过时）", t.format("%Y-%m-%d %H:%M:%S"))
                } else {
                    label
                }
            }
        },
    ));
    out
}

/// 判断题目是否过时：检查关键文件是否在 last_tested 之后被修改。
fn is_stale(contest_dir: &Path, pid: &str, last_tested: chrono::DateTime<chrono::Utc>) -> bool {
    let pdir = problem_dir(contest_dir, pid);
    let check_paths = [
        pdir.join("config.yaml"),
        pdir.join("auxiliary"),
        pdir.join("solutions"),
        pdir.join("data"),
    ];
    let last_modified = last_tested.timestamp();
    for path in &check_paths {
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(mtime) = meta.modified()
                && let Ok(dt) = mtime.duration_since(std::time::UNIX_EPOCH)
                    && dt.as_secs() as i64 > last_modified {
                        return true;
                    }
            // 目录递归检查
            if meta.is_dir()
                && let Ok(rd) = std::fs::read_dir(path) {
                    for e in rd.flatten() {
                        if let Ok(m) = e.metadata()
                            && let Ok(mt) = m.modified()
                                && let Ok(dt) = mt.duration_since(std::time::UNIX_EPOCH)
                                    && dt.as_secs() as i64 > last_modified {
                                        return true;
                                    }
                    }
                }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

pub fn sanitize_id(id: &str) -> Result<()> {
    if id.is_empty() {
        bail!("id 不能为空");
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains(char::is_whitespace)
    {
        bail!("id 不能包含路径分隔符、'..' 或空白字符");
    }
    if id == CONFIG_NAME || id == "data" || id == "statement" || id == "auxiliary" || id == "solutions"
    {
        bail!("id 不能使用保留名");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, ProblemType, Verdict};

    fn tmp_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("preparer_test_{}_{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn init_and_load_contest() {
        let d = tmp_dir("init");
        let c = init_contest(&d, "test").unwrap();
        assert_eq!(c.name, "test");
        let loaded = load_contest(&d).unwrap();
        assert_eq!(loaded.name, "test");
        assert!(loaded.problems.is_empty());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn add_problem_creates_layout() {
        let d = tmp_dir("addprob");
        init_contest(&d, "c").unwrap();
        let p = add_problem(
            &d,
            NewProblem {
                id: "a",
                name: Some("A+B"),
                problem_type: Some(ProblemType::Traditional),
                source: Some(ProblemSource::Original),
            },
        )
        .unwrap();
        assert_eq!(p.id, "a");
        assert!(d.join("a").join("statement").is_dir());
        assert!(d.join("a").join("statement").join("down").is_dir());
        assert!(d.join("a").join("data").is_dir());
        assert!(!d.join("a").join("data").join("config.yaml").exists()); // subtasks 在题目 config.yaml 中
        assert!(d.join("a").join("auxiliary").is_dir());
        assert!(d.join("a").join("solutions").is_dir());
        // subtasks 与 data_gen 在题目 config.yaml 中
        let loaded_p = load_problem(&d.join("a")).unwrap();
        assert!(loaded_p.subtasks.is_empty());
        assert!(loaded_p.data_gen.is_empty());
        let loaded = load_contest(&d).unwrap();
        assert_eq!(loaded.problems, vec!["a"]);
        assert_eq!(loaded.loaded_problems.len(), 1);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn set_component_status_persists() {
        let d = tmp_dir("setcomp");
        init_contest(&d, "c").unwrap();
        add_problem(
            &d,
            NewProblem {
                id: "a",
                name: None,
                problem_type: None,
                source: None,
            },
        )
        .unwrap();
        set_component_status(&d, "a", "statement", ComponentStatus::completed_now()).unwrap();
        let p = load_problem(&problem_dir(&d, "a")).unwrap();
        assert!(p.statement.is_terminal_ok());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn add_solution_and_sol_status() {
        let d = tmp_dir("addsol");
        init_contest(&d, "c").unwrap();
        add_problem(
            &d,
            NewProblem {
                id: "a",
                name: None,
                problem_type: None,
                source: None,
            },
        )
        .unwrap();
        add_solution(
            &d,
            "a",
            SolutionStatus {
                name: "brute".into(),
                file: Some("solutions/brute.cpp".into()),
                expected: model::JudgingStatus {
                    verdict: Verdict::Wa,
                    score: Some(30.0),
                },
                status: ComponentStatus::NotStarted,
            },
        )
        .unwrap();
        set_component_status(&d, "a", "sol:brute", ComponentStatus::completed_now()).unwrap();
        let p = load_problem(&problem_dir(&d, "a")).unwrap();
        assert_eq!(p.sols.len(), 1);
        assert!(p.sols[0].status.is_terminal_ok());
        // aggregate
        set_component_status(&d, "a", "sols", ComponentStatus::completed_now()).unwrap();
        let p = load_problem(&problem_dir(&d, "a")).unwrap();
        assert!(p.sols[0].status.is_terminal_ok());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn resolve_problem_single_default() {
        let d = tmp_dir("resolve");
        init_contest(&d, "c").unwrap();
        add_problem(
            &d,
            NewProblem {
                id: "a",
                name: None,
                problem_type: None,
                source: None,
            },
        )
        .unwrap();
        assert_eq!(resolve_problem_id(&d, None).unwrap(), "a");
        assert_eq!(resolve_problem_id(&d, Some("a")).unwrap(), "a");
        assert!(resolve_problem_id(&d, Some("nope")).is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn sanitize_rejects_bad() {
        assert!(sanitize_id("a b").is_err());
        assert!(sanitize_id("../x").is_err());
        assert!(sanitize_id("data").is_err());
        assert!(sanitize_id("ok_id-1").is_ok());
    }
}
