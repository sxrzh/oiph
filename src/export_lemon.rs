//! 导出为 LemonLime 格式。
//!
//! 生成：
//! - `<output>/<contest_name>.cdf`：比赛配置 JSON
//! - `<output>/data/<problem_id>/`：每题的测试数据、SPJ、grader 等
//! - `<output>/compile_spj.bat`：编译所有 SPJ 的批处理脚本

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::model::{Problem, ProblemType, SubtaskType};
use crate::project;

/// 导出当前比赛为 LemonLime 格式。
///
/// `output_dir` 为 None 时默认 `<contest_dir>/<contest_name>_lemon/`。
/// 返回（输出目录，警告列表）。LemonLime 只支持函数交互题（interactive_lib / function），
/// 遇到 IO 交互题（interactive_io）会跳过并记录警告，其他题目正常导出。
pub fn export(contest_dir: &Path, output_dir: Option<&Path>) -> Result<(PathBuf, Vec<String>)> {
    // 导出前先检查 vendor/testlib_lemon.h 存在（SPJ 导出需要）
    let lemon_testlib = crate::paths::vendor_dir().join("testlib_lemon.h");
    anyhow::ensure!(
        lemon_testlib.is_file(),
        "缺少 {}（LemonLime SPJ 导出需要），请先运行 init.sh 初始化",
        lemon_testlib.display()
    );
    let contest = project::load_contest(contest_dir)?;
    let out = output_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| contest_dir.join(format!("{}_lemon", sanitize(&contest.name))));
    std::fs::create_dir_all(&out)
        .with_context(|| format!("创建输出目录 {} 失败", out.display()))?;
    let data_dir = out.join("data");
    std::fs::create_dir_all(&data_dir)?;

    let mut tasks = Vec::new();
    let mut spj_dirs: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let source_dir = out.join("source");
    let std_dir = source_dir.join("std");

    for pid in &contest.problems {
        let problem = project::load_problem(&project::problem_dir(contest_dir, pid))?;
        // LemonLime 只支持函数交互题；IO 交互题跳过并报告，其他题正常导出
        if problem.problem_type == ProblemType::InteractiveIO {
            warnings.push(format!(
                "题目 {pid} 为 IO 交互题（interactive_io），LemonLime 不支持，已跳过"
            ));
            continue;
        }
        let task = build_task(&problem, contest_dir, &data_dir, &std_dir, &mut spj_dirs, &mut warnings)?;
        tasks.push(task);
    }

    // 生成 CDF
    let cdf = json!({
        "contestTitle": contest.name,
        "contestants": [],
        "tasks": tasks,
    });
    let cdf_path = out.join(format!("{}.cdf", sanitize(&contest.name)));
    std::fs::write(&cdf_path, serde_json::to_string_pretty(&cdf)?)
        .with_context(|| format!("写入 {} 失败", cdf_path.display()))?;

    // 生成 compile_spj.bat
    if !spj_dirs.is_empty() {
        write_compile_bat(&out, &spj_dirs)?;
    }

    Ok((out, warnings))
}

fn sanitize(name: &str) -> String {
    name.replace(char::is_whitespace, "_")
}

/// 构建单个题目的 LemonLime task JSON，同时准备数据文件。
fn build_task(
    problem: &Problem,
    contest_dir: &Path,
    data_dir: &Path,
    std_dir: &Path,
    spj_dirs: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<Value> {
    let pid = &problem.id;
    let pdata_dir = data_dir.join(pid);
    std::fs::create_dir_all(&pdata_dir)?;

    // 准备测试数据：data_gen 配置的现场生成，其余从 data/ 复制
    prepare_data(problem, contest_dir, &pdata_dir, warnings)?;

    // 拷贝 down/ 里的下发文件
    let down_dir = project::problem_dir(contest_dir, pid).join("statement").join("down");
    if down_dir.exists() {
        copy_dir_contents(&down_dir, &pdata_dir)?;
    }

    // 拷贝 std 到 source/std/<pid>/<pid>.cpp
    let pdir = project::problem_dir(contest_dir, pid);
    let std_src = problem.std.file.as_deref().unwrap_or("solutions/std.cpp");
    let std_path = pdir.join(std_src);
    if std_path.exists() {
        let std_subdir = std_dir.join(pid);
        std::fs::create_dir_all(&std_subdir)?;
        std::fs::copy(&std_path, std_subdir.join(format!("{pid}.cpp")))?;
    }

    // 判断是否有 SPJ
    let has_checker = problem.checker.status.is_terminal_ok();
    let checker_path = project::problem_dir(contest_dir, pid)
        .join("auxiliary")
        .join("checker.cpp");
    let is_spj = has_checker && checker_path.exists();

    // 判断题目类型（函数交互题：interactive_lib / function；IO 交互题已在 export 中跳过）
    let is_interactive = problem.problem_type == ProblemType::Function;

    // comparisonMode: 1=普通比较, 4=SPJ
    let comparison_mode = if is_spj { 4 } else { 1 };

    // SPJ 处理：拷贝 checker.cpp → spj.cpp，拷贝 lemon testlib.h
    let mut special_judge = String::new();
    if is_spj {
        let spj_dst = pdata_dir.join("spj.cpp");
        // 读取 checker.cpp，替换 registerTestlibCmd → registerLemonChecker，写入 spj.cpp
        let checker_src = std::fs::read_to_string(&checker_path)
            .with_context(|| format!("读取 checker.cpp 失败：{}", checker_path.display()))?;
        let spj_src = checker_src.replace("registerTestlibCmd", "registerLemonChecker");
        std::fs::write(&spj_dst, &spj_src)
            .with_context(|| format!("写入 spj.cpp 失败：{}", spj_dst.display()))?;
        // 拷贝 lemon 兼容的 testlib.h（~/.oiph/vendor/testlib_lemon.h，导出前已检查存在）
        let lemon_h = crate::paths::vendor_read("testlib_lemon.h")?;
        std::fs::write(pdata_dir.join("testlib.h"), lemon_h.as_bytes())?;
        special_judge = format!("{pid}/spj.exe");
        spj_dirs.push(pid.clone());
    }

    // 交互题处理
    let mut grader = serde_json::Value::Null;
    let mut interactor = serde_json::Value::Null;
    let mut interactor_name = serde_json::Value::Null;
    let task_type = if is_interactive { 2 } else { 0 };

    if is_interactive {
        let aux_dir = project::problem_dir(contest_dir, pid).join("auxiliary");
        // interactive_lib.cpp → grader.cpp
        let lib_path = aux_dir.join("interactive_lib.cpp");
        if lib_path.exists() {
            let dst = pdata_dir.join("grader.cpp");
            std::fs::copy(&lib_path, &dst)?;
            grader = json!(format!("{pid}/grader.cpp"));
        }
        // 如果有 {pid}.h
        let inter_h = aux_dir.join(format!("{pid}.h"));
        if inter_h.exists() {
            std::fs::copy(&inter_h, pdata_dir.join(format!("{pid}.h")))?;
            interactor = json!(format!("{pid}/{pid}.h"));
            interactor_name = json!(format!("{pid}.h"));
        }
    }

    // 构建测试点列表
    let test_cases = build_test_cases(problem, pid)?;

    let mut task = json!({
        "answerFileExtension": "out",
        "comparisonMode": comparison_mode,
        "compilerConfiguration": {"g++": "default", "gcc": "default"},
        "diffArguments": "--ignore-space-change --text --brief",
        "inputFileName": format!("{pid}.in"),
        "outputFileName": format!("{pid}.out"),
        "problemTitle": if problem.name.is_empty() { pid.clone() } else { problem.name.clone() },
        "realPrecision": 3,
        "sourceFileName": pid,
        "specialJudge": special_judge,
        "standardInputCheck": true,
        "standardOutputCheck": true,
        "subFolderCheck": true,
        "taskType": task_type,
        "testCases": test_cases,
    });

    if is_interactive {
        let obj = task.as_object_mut().unwrap();
        obj.insert("grader".into(), grader);
        obj.insert("interactor".into(), interactor);
        obj.insert("interactorName".into(), interactor_name);
    }

    Ok(task)
}

/// 从 subtasks 构建测试点列表，处理依赖关系。
fn build_test_cases(problem: &Problem, pid: &str) -> Result<Vec<Value>> {
    if problem.subtasks.is_empty() {
        // 无 subtasks 配置：自动发现 data/ 下的 .in/.ans 对
        return auto_discover_cases(problem, pid);
    }

    // 展开所有测试点，记录每个 case 属于哪个 subtask（1-based）
    struct FlatCase {
        name: String,
        full_score: i64,
        depend_subtasks: Vec<u32>,
    }
    let mut flat: Vec<FlatCase> = Vec::new();
    let mut subtask_case_ranges: BTreeMap<u32, Vec<usize>> = BTreeMap::new();

    for (i, st) in problem.subtasks.iter().enumerate() {
        let subtask_idx = (i + 1) as u32;
        let cases = &st.cases;
        if cases.is_empty() {
            continue;
        }
        let per_case_score = match st.stype {
            SubtaskType::Sum => (st.score / cases.len() as f64) as i64,
            SubtaskType::Min | SubtaskType::Mul => st.score as i64,
        };
        for case_name in cases {
            let fc = FlatCase {
                name: case_name.clone(),
                full_score: per_case_score,
                depend_subtasks: st.depend.clone(),
            };
            let case_idx = flat.len();
            flat.push(fc);
            subtask_case_ranges
                .entry(subtask_idx)
                .or_default()
                .push(case_idx);
        }
    }

    let mut result = Vec::new();
    for (case_idx, fc) in flat.iter().enumerate() {
        let _case_num = case_idx + 1;
        let mut input_files = vec![format!("{pid}/{}.in", fc.name)];

        // 添加依赖标志
        for dep_subtask in &fc.depend_subtasks {
            if let Some(case_indices) = subtask_case_ranges.get(dep_subtask) {
                for &dep_case_idx in case_indices {
                    let dep_case_num = dep_case_idx + 1;
                    input_files.push(format!(
                        "{dep_case_num}_lemon_SUbtaskDEPENDENCE_fLAg"
                    ));
                }
            }
        }

        let output_files = vec![format!("{}\\{}.ans", pid, fc.name)];

        result.push(json!({
            "fullScore": fc.full_score,
            "inputFiles": input_files,
            "memoryLimit": problem.memory_limit_mb,
            "outputFiles": output_files,
            "timeLimit": problem.time_limit_ms,
        }));
    }

    Ok(result)
}

/// 无 subtasks 时自动发现测试数据。
fn auto_discover_cases(problem: &Problem, pid: &str) -> Result<Vec<Value>> {
    // 这个函数在导出时被调用，但没有 subtasks 配置
    // 返回空列表；用户应先配置 subtasks
    eprintln!("[export_lemon] 警告：题目 {pid} 没有配置 subtasks，测试点列表为空");
    let _ = problem;
    Ok(vec![])
}

/// 带超时的命令（用 `timeout <secs>` 包裹，防止 generator/std 意外挂起）。
fn timed_cmd(program: &Path, secs: u64) -> Command {
    let mut c = Command::new("timeout");
    c.arg(secs.to_string()).arg(program);
    c
}

/// 按题目配置准备测试数据到导出目录：
/// - `data_gen` 中配置的测试点：现场编译并调用 generator 生成 `.in`
/// - 其余测试点：从 `data/` 复制 `.in`（缺失则告警）
/// - `.ans`：优先从 `data/` 复制；缺失时传统题用 std 现场生成
fn prepare_data(
    problem: &Problem,
    contest_dir: &Path,
    pdata_dir: &Path,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let pid = &problem.id;
    let pdir = project::problem_dir(contest_dir, pid);
    let src_data_dir = pdir.join("data");
    let aux_dir = pdir.join("auxiliary");

    // 测试点列表：subtasks 的 cases（去重保序）；无配置则发现 data/*.in
    let mut cases: Vec<String> = Vec::new();
    for st in &problem.subtasks {
        for c in &st.cases {
            if !cases.contains(c) {
                cases.push(c.clone());
            }
        }
    }
    if cases.is_empty()
        && let Ok(rd) = std::fs::read_dir(&src_data_dir)
    {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".in") {
                cases.push(stem.to_string());
            }
        }
        cases.sort();
    }

    let tmp = std::env::temp_dir().join(format!("oiph_export_{pid}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp)?;

    // 需要生成数据：编译 generator
    let need_gen = cases.iter().any(|c| problem.data_gen.contains_key(c));
    let mut gen_bin: Option<PathBuf> = None;
    if need_gen {
        let testlib_h = aux_dir.join("testlib.h");
        if !testlib_h.exists()
            && let Ok(content) = crate::paths::vendor_read("testlib.h")
        {
            let _ = std::fs::write(&testlib_h, content.as_bytes());
        }
        let src = aux_dir.join("generator.cpp");
        if !src.exists() {
            warnings.push(format!("题目 {pid}：配置了生成数据但缺少 auxiliary/generator.cpp"));
        } else {
            let flags = problem.compile_flags.split_whitespace().collect::<Vec<_>>();
            let out = tmp.join("generator");
            match timed_cmd(Path::new("g++"), 600)
                .args(&flags)
                .arg("-I")
                .arg(&aux_dir)
                .arg("-o")
                .arg(&out)
                .arg(&src)
                .stderr(Stdio::piped())
                .output()
            {
                Ok(o) if o.status.success() => gen_bin = Some(out),
                Ok(o) => warnings.push(format!(
                    "题目 {pid}：generator 编译失败，生成的测试点将回退为复制 data/：\n{}",
                    String::from_utf8_lossy(&o.stderr)
                )),
                Err(e) => warnings.push(format!("题目 {pid}：generator 编译失败：{e}")),
            }
        }
    }

    // 答案缺失的传统题：编译 std 现场生成 .ans
    let mut std_bin: Option<PathBuf> = None;
    if problem.problem_type == ProblemType::Traditional {
        let missing_ans = cases
            .iter()
            .any(|c| !src_data_dir.join(format!("{c}.ans")).is_file());
        if missing_ans {
            let std_src = problem.std.file.as_deref().unwrap_or("solutions/std.cpp");
            let std_path = pdir.join(std_src);
            if !std_path.exists() {
                warnings.push(format!("题目 {pid}：缺少 std（{std_src}），缺失的答案无法现场生成"));
            } else {
                let flags = problem.compile_flags.split_whitespace().collect::<Vec<_>>();
                let out = tmp.join("std");
                match timed_cmd(Path::new("g++"), 600)
                    .args(&flags)
                    .arg("-o")
                    .arg(&out)
                    .arg(&std_path)
                    .stderr(Stdio::piped())
                    .output()
                {
                    Ok(o) if o.status.success() => std_bin = Some(out),
                    Ok(o) => warnings.push(format!(
                        "题目 {pid}：std 编译失败，缺失的答案无法现场生成：\n{}",
                        String::from_utf8_lossy(&o.stderr)
                    )),
                    Err(e) => warnings.push(format!("题目 {pid}：std 编译失败：{e}")),
                }
            }
        }
    }

    // 逐个测试点：生成或复制 .in；复制或生成 .ans
    for case in &cases {
        let in_src = src_data_dir.join(format!("{case}.in"));
        let in_dst = pdata_dir.join(format!("{case}.in"));
        let ans_src = src_data_dir.join(format!("{case}.ans"));
        let ans_dst = pdata_dir.join(format!("{case}.ans"));

        if problem.data_gen.contains_key(case) {
            // 生成的测试点：现场调用 generator（失败回退复制 data/）
            let mut generated = false;
            if let Some(gen_path) = &gen_bin {
                let args = problem
                    .data_gen
                    .get(case)
                    .map(String::as_str)
                    .unwrap_or("");
                let output = timed_cmd(gen_path, 60)
                    .args(args.split_whitespace())
                    .stdout(
                        std::fs::File::create(&in_dst)
                            .map(Stdio::from)
                            .unwrap_or(Stdio::null()),
                    )
                    .stderr(Stdio::piped())
                    .output();
                match output {
                    Ok(o) if o.status.success() => generated = true,
                    Ok(o) => warnings.push(format!(
                        "题目 {pid}：generator 生成 {case} 失败：{}",
                        String::from_utf8_lossy(&o.stderr)
                    )),
                    Err(e) => warnings.push(format!("题目 {pid}：generator 运行失败（{case}）：{e}")),
                }
            }
            if !generated {
                if in_src.is_file() {
                    std::fs::copy(&in_src, &in_dst)?;
                    warnings.push(format!(
                        "题目 {pid}：测试点 {case} 生成失败，已回退为复制 data/{case}.in"
                    ));
                } else {
                    warnings.push(format!(
                        "题目 {pid}：测试点 {case} 生成失败且 data/ 中无 {case}.in"
                    ));
                }
            }
        } else if in_src.is_file() {
            std::fs::copy(&in_src, &in_dst)
                .with_context(|| format!("复制 {} 失败", in_src.display()))?;
        } else {
            warnings.push(format!(
                "题目 {pid}：测试点 {case} 缺少 data/{case}.in（且未配置生成参数）"
            ));
        }

        if ans_src.is_file() {
            std::fs::copy(&ans_src, &ans_dst)
                .with_context(|| format!("复制 {} 失败", ans_src.display()))?;
        } else if let Some(std) = &std_bin {
            if in_dst.is_file() {
                let timeout_secs = (problem.time_limit_ms / 1000).max(5) * 3;
                let output = timed_cmd(std, timeout_secs)
                    .stdin(
                        std::fs::File::open(&in_dst)
                            .map(Stdio::from)
                            .unwrap_or(Stdio::null()),
                    )
                    .stdout(
                        std::fs::File::create(&ans_dst)
                            .map(Stdio::from)
                            .unwrap_or(Stdio::null()),
                    )
                    .stderr(Stdio::piped())
                    .output();
                match output {
                    Ok(o) if o.status.success() => {}
                    Ok(o) => warnings.push(format!(
                        "题目 {pid}：std 生成 {case}.ans 失败（退出码 {:?}）",
                        o.status.code()
                    )),
                    Err(e) => warnings.push(format!("题目 {pid}：std 运行失败（{case}）：{e}")),
                }
            }
        } else if problem.problem_type != ProblemType::Traditional {
            warnings.push(format!(
                "题目 {pid}：测试点 {case} 缺少 data/{case}.ans（该题型无法现场生成答案）"
            ));
        } else {
            warnings.push(format!(
                "题目 {pid}：测试点 {case} 缺少答案（data/{case}.ans 不存在且 std 不可用）"
            ));
        }

        // 兼容 data/ 中已有的 <case>.out（原样保留）
        let out_src = src_data_dir.join(format!("{case}.out"));
        if out_src.is_file() {
            std::fs::copy(&out_src, pdata_dir.join(format!("{case}.out")))
                .with_context(|| format!("复制 {} 失败", out_src.display()))?;
        }
    }

    // 复制 data/ 中未被 cases 覆盖的其余 .in/.ans/.out（保留额外文件）
    if let Ok(rd) = std::fs::read_dir(&src_data_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some((stem, _)) = name.rsplit_once('.') else {
                continue;
            };
            if cases.iter().any(|c| c == stem) {
                continue;
            }
            if name.ends_with(".in") || name.ends_with(".ans") || name.ends_with(".out") {
                let _ = std::fs::copy(e.path(), pdata_dir.join(&name));
            }
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// 递归拷贝目录内容。
fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let p = e.path();
        let target = dst.join(e.file_name());
        if p.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir_contents(&p, &target)?;
        } else {
            std::fs::copy(&p, &target)?;
        }
    }
    Ok(())
}

/// 生成 compile_spj.bat。
fn write_compile_bat(out_dir: &Path, spj_dirs: &[String]) -> Result<()> {
    let mut content = String::from("@echo off\r\n");
    content.push_str("echo Compiling SPJ files...\r\n");
    for pid in spj_dirs {
        content.push_str(&format!(
            "if exist \"data\\{pid}\\spj.cpp\" (\r\n  echo Compiling {pid}\\spj.cpp...\r\n  g++ -O2 -std=c++14 -o \"data\\{pid}\\spj.exe\" \"data\\{pid}\\spj.cpp\" -I\"data\\{pid}\"\r\n)\r\n"
        ));
    }
    content.push_str("echo Done.\r\npause\r\n");
    let path = out_dir.join("compile_spj.bat");
    std::fs::write(&path, content)
        .with_context(|| format!("写入 {} 失败", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComponentStatus, Subtask, SubtaskType};
    // use chrono::Utc;

    fn make_contest(dir: &Path) {
        project::init_contest(dir, "test").unwrap();
    }

    fn make_problem(contest_dir: &Path, id: &str) {
        project::add_problem(
            contest_dir,
            project::NewProblem {
                id,
                name: Some(id),
                problem_type: None,
                source: None,
            },
        )
        .unwrap();
    }

    fn write_data(contest_dir: &Path, pid: &str, name: &str, content: &str) {
        let pdir = project::problem_dir(contest_dir, pid);
        std::fs::create_dir_all(pdir.join("data")).unwrap();
        std::fs::write(pdir.join("data").join(format!("{name}.in")), content).unwrap();
        std::fs::write(pdir.join("data").join(format!("{name}.ans")), content).unwrap();
    }

    fn set_subtasks(contest_dir: &Path, pid: &str, subtasks: Vec<Subtask>) {
        project::with_problem_mut(contest_dir, pid, |p| {
            p.subtasks = subtasks;
            Ok(())
        })
        .unwrap();
    }

    fn has_gpp() -> bool {
        Command::new("g++")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn export_generates_configured_cases() {
        if !has_gpp() {
            eprintln!("跳过：未找到 g++");
            return;
        }
        let _home = crate::paths::tests::sandbox_home_with_vendor("lemon_gen");
        let dir = std::env::temp_dir().join(format!("prep_lemon_gen_{}", uuid::Uuid::new_v4()));
        make_contest(&dir);
        make_problem(&dir, "g");
        let pdir = project::problem_dir(&dir, "g");

        // generator：输出固定内容
        std::fs::create_dir_all(pdir.join("auxiliary")).unwrap();
        std::fs::write(
            pdir.join("auxiliary/generator.cpp"),
            "#include <cstdio>\nint main(){ printf(\"GENERATED\\n\"); }\n",
        )
        .unwrap();

        // 点 2：从 data/ 复制；点 1：现场生成输入 + data/ 提供答案；
        // 点 3：data/ 提供输入、缺失答案 → 用 std 现场生成
        write_data(&dir, "g", "2", "copied");
        std::fs::create_dir_all(pdir.join("data")).unwrap();
        std::fs::write(pdir.join("data/1.ans"), "gen-ans").unwrap();
        std::fs::write(pdir.join("data/3.in"), "3 4\n").unwrap();
        std::fs::create_dir_all(pdir.join("solutions")).unwrap();
        std::fs::write(
            pdir.join("solutions/std.cpp"),
            "#include <cstdio>\nint main(){ int a,b; if(scanf(\"%d %d\",&a,&b)!=2) return 1; printf(\"%d\\n\", a+b); }\n",
        )
        .unwrap();

        set_subtasks(
            &dir,
            "g",
            vec![Subtask {
                score: 100.0,
                stype: SubtaskType::Sum,
                cases: vec!["1".into(), "2".into(), "3".into()],
                pretest: false,
                sample: false,
                depend: vec![],
            }],
        );
        project::with_problem_mut(&dir, "g", |p| {
            p.data_gen.insert("1".into(), String::new());
            Ok(())
        })
        .unwrap();

        let (out, warnings) = export(&dir, None).unwrap();
        assert!(warnings.is_empty(), "意外警告：{warnings:?}");
        let d = out.join("data").join("g");
        assert!(
            std::fs::read_to_string(d.join("1.in")).unwrap().contains("GENERATED"),
            "生成点 1 的输入应来自 generator"
        );
        assert_eq!(std::fs::read_to_string(d.join("2.in")).unwrap(), "copied");
        assert_eq!(std::fs::read_to_string(d.join("1.ans")).unwrap(), "gen-ans");
        assert_eq!(std::fs::read_to_string(d.join("3.ans")).unwrap(), "7\n");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn export_basic_contest() {
        let _home = crate::paths::tests::sandbox_home_with_vendor("lemon_basic");
        let dir = std::env::temp_dir().join(format!("prep_lemon_{}", uuid::Uuid::new_v4()));
        make_contest(&dir);
        make_problem(&dir, "a");
        write_data(&dir, "a", "1", "42");
        write_data(&dir, "a", "2", "hello");
        set_subtasks(
            &dir,
            "a",
            vec![
                Subtask {
                    score: 30.0,
                    stype: SubtaskType::Sum,
                    cases: vec!["1".into(), "2".into()],
                    pretest: false,
                    sample: false,
                    depend: vec![],
                },
                Subtask {
                    score: 70.0,
                    stype: SubtaskType::Sum,
                    cases: vec!["3".into()],
                    pretest: false,
                    sample: false,
                    depend: vec![1],
                },
            ],
        );
        write_data(&dir, "a", "3", "world");

        let (out, warnings) = export(&dir, None).unwrap();
        assert!(warnings.is_empty());
        assert!(out.join("test.cdf").is_file());
        assert!(out.join("data").join("a").join("1.in").is_file());
        assert!(out.join("data").join("a").join("1.ans").is_file());
        assert!(out.join("data").join("a").join("3.in").is_file());

        let cdf: Value = serde_json::from_str(
            &std::fs::read_to_string(out.join("test.cdf")).unwrap(),
        )
        .unwrap();
        assert_eq!(cdf["contestTitle"], "test");
        let tasks = cdf["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        let tcs = tasks[0]["testCases"].as_array().unwrap();
        assert_eq!(tcs.len(), 3);
        assert_eq!(tcs[0]["fullScore"], 15); // 30/2
        assert_eq!(tcs[1]["fullScore"], 15);
        assert_eq!(tcs[2]["fullScore"], 70);
        // 第三个测试点依赖 subtask 1（即 test case 1 和 2）
        let inputs = tcs[2]["inputFiles"].as_array().unwrap();
        assert!(inputs.len() >= 2); // a/3.in + dependence flags
        assert!(inputs.iter().any(|v| v.as_str().unwrap().contains("1_lemon_SUbtaskDEPENDENCE_fLAg")));
        assert!(inputs.iter().any(|v| v.as_str().unwrap().contains("2_lemon_SUbtaskDEPENDENCE_fLAg")));
        // 无 SPJ → 无 bat
        assert!(!out.join("compile_spj.bat").exists());

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn export_with_spj() {
        let _home = crate::paths::tests::sandbox_home_with_vendor("lemon_spj");
        let dir = std::env::temp_dir().join(format!("prep_lemon_spj_{}", uuid::Uuid::new_v4()));
        make_contest(&dir);
        make_problem(&dir, "b");
        write_data(&dir, "b", "1", "x");
        set_subtasks(
            &dir,
            "b",
            vec![Subtask {
                score: 100.0,
                stype: SubtaskType::Sum,
                cases: vec!["1".into()],
                pretest: false,
                sample: false,
                depend: vec![],
            }],
        );
        // 写 checker.cpp 并标记完成
        let pdir = project::problem_dir(&dir, "b");
        std::fs::create_dir_all(pdir.join("auxiliary")).unwrap();
        std::fs::write(pdir.join("auxiliary").join("checker.cpp"), "int main(){}").unwrap();
        project::set_component_status(&dir, "b", "checker", ComponentStatus::completed_now())
            .unwrap();

        let (out, warnings) = export(&dir, None).unwrap();
        assert!(warnings.is_empty());
        // SPJ 文件
        assert!(out.join("data").join("b").join("spj.cpp").is_file());
        assert!(out.join("data").join("b").join("testlib.h").is_file());
        // compile_spj.bat
        assert!(out.join("compile_spj.bat").is_file());
        let bat = std::fs::read_to_string(out.join("compile_spj.bat")).unwrap();
        assert!(bat.contains("b"));
        assert!(bat.contains("g++"));

        let cdf: Value = serde_json::from_str(
            &std::fs::read_to_string(out.join("test.cdf")).unwrap(),
        )
        .unwrap();
        assert_eq!(cdf["tasks"][0]["comparisonMode"], 4);
        assert_eq!(cdf["tasks"][0]["specialJudge"], "b/spj.exe");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn export_interactive() {
        let _home = crate::paths::tests::sandbox_home_with_vendor("lemon_inter");
        let dir = std::env::temp_dir().join(format!("prep_lemon_inter_{}", uuid::Uuid::new_v4()));
        project::init_contest(&dir, "inter_test").unwrap();
        project::add_problem(
            &dir,
            project::NewProblem {
                id: "c",
                name: Some("C"),
                problem_type: Some(ProblemType::Function),
                source: None,
            },
        )
        .unwrap();
        write_data(&dir, "c", "1", "data");
        set_subtasks(
            &dir,
            "c",
            vec![Subtask {
                score: 100.0,
                stype: SubtaskType::Sum,
                cases: vec!["1".into()],
                pretest: false,
                sample: false,
                depend: vec![],
            }],
        );
        let pdir = project::problem_dir(&dir, "c");
        std::fs::create_dir_all(pdir.join("auxiliary")).unwrap();
        std::fs::write(pdir.join("auxiliary").join("interactive_lib.cpp"), "int main(){}").unwrap();
        std::fs::write(pdir.join("auxiliary").join("c.h"), "#pragma once").unwrap();

        let (out, warnings) = export(&dir, None).unwrap();
        assert!(warnings.is_empty());
        let cdf: Value = serde_json::from_str(
            &std::fs::read_to_string(out.join("inter_test.cdf")).unwrap(),
        )
        .unwrap();
        assert_eq!(cdf["tasks"][0]["taskType"], 2);
        assert_eq!(cdf["tasks"][0]["grader"], "c/grader.cpp");
        assert_eq!(cdf["tasks"][0]["interactor"], "c/c.h");
        assert_eq!(cdf["tasks"][0]["interactorName"], "c.h");
        assert!(out.join("data").join("c").join("grader.cpp").is_file());
        assert!(out.join("data").join("c").join("c.h").is_file());

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn export_skips_io_interactive_with_warning() {
        let _home = crate::paths::tests::sandbox_home_with_vendor("lemon_io");
        let dir = std::env::temp_dir().join(format!("prep_lemon_io_{}", uuid::Uuid::new_v4()));
        project::init_contest(&dir, "io_test").unwrap();
        // 传统题 a
        make_problem(&dir, "a");
        write_data(&dir, "a", "1", "42");
        set_subtasks(
            &dir,
            "a",
            vec![Subtask {
                score: 100.0,
                stype: SubtaskType::Sum,
                cases: vec!["1".into()],
                pretest: false,
                sample: false,
                depend: vec![],
            }],
        );
        // IO 交互题 b
        project::add_problem(
            &dir,
            project::NewProblem {
                id: "b",
                name: Some("B"),
                problem_type: Some(ProblemType::InteractiveIO),
                source: None,
            },
        )
        .unwrap();

        let (out, warnings) = export(&dir, None).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("b") && warnings[0].contains("IO 交互"));
        // 其他题正常导出
        let cdf: Value = serde_json::from_str(
            &std::fs::read_to_string(out.join("io_test.cdf")).unwrap(),
        )
        .unwrap();
        let tasks = cdf["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["problemTitle"], "a");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
    }
}
