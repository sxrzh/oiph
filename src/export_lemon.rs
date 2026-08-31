//! 导出为 LemonLime 格式。
//!
//! 生成：
//! - `<output>/<contest_name>.cdf`：比赛配置 JSON
//! - `<output>/data/<problem_id>/`：每题的测试数据、SPJ、grader 等
//! - `<output>/compile_spj.bat`：编译所有 SPJ 的批处理脚本

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::model::{Problem, ProblemType, SubtaskType};
use crate::project;

/// 导出当前比赛为 LemonLime 格式。
///
/// `output_dir` 为 None 时默认 `<contest_dir>/<contest_name>_lemon/`。
pub fn export(contest_dir: &Path, output_dir: Option<&Path>) -> Result<PathBuf> {
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

    for pid in &contest.problems {
        let problem = project::load_problem(&project::problem_dir(contest_dir, pid))?;
        let task = build_task(&problem, contest_dir, &data_dir, &mut spj_dirs)?;
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

    Ok(out)
}

fn sanitize(name: &str) -> String {
    name.replace(char::is_whitespace, "_")
}

/// 构建单个题目的 LemonLime task JSON，同时拷贝数据文件。
fn build_task(
    problem: &Problem,
    contest_dir: &Path,
    data_dir: &Path,
    spj_dirs: &mut Vec<String>,
) -> Result<Value> {
    let pid = &problem.id;
    let pdata_dir = data_dir.join(pid);
    std::fs::create_dir_all(&pdata_dir)?;

    // 拷贝测试数据
    let src_data_dir = project::problem_dir(contest_dir, pid).join("data");
    copy_data_files(&src_data_dir, &pdata_dir)?;

    // 拷贝 down/ 里的下发文件
    let down_dir = project::problem_dir(contest_dir, pid).join("statement").join("down");
    if down_dir.exists() {
        copy_dir_contents(&down_dir, &pdata_dir)?;
    }

    // 判断是否有 SPJ
    let has_checker = problem.checker.status.is_terminal_ok();
    let checker_path = project::problem_dir(contest_dir, pid)
        .join("auxiliary")
        .join("checker.cpp");
    let is_spj = has_checker && checker_path.exists();

    // 判断题目类型
    let is_interactive = matches!(
        problem.problem_type,
        ProblemType::InteractiveLib | ProblemType::InteractiveIO
    );

    // comparisonMode: 1=普通比较, 4=SPJ
    let comparison_mode = if is_spj { 4 } else { 1 };

    // SPJ 处理：拷贝 checker.cpp → spj.cpp，拷贝 lemon testlib.h
    let mut special_judge = String::new();
    if is_spj {
        let spj_dst = pdata_dir.join("spj.cpp");
        std::fs::copy(&checker_path, &spj_dst)
            .with_context(|| format!("拷贝 checker.cpp 到 {} 失败", spj_dst.display()))?;
        // 拷贝 lemon 兼容的 testlib.h
        std::fs::write(pdata_dir.join("testlib.h"), crate::assets::LEMON_TESTLIB_H)?;
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
        "subFolderCheck": false,
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

/// 拷贝 data/ 下的所有 .in 和 .ans 文件。
fn copy_data_files(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".in") || name.ends_with(".ans") || name.ends_with(".out") {
            std::fs::copy(&p, dst.join(e.file_name()))?;
        }
    }
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
    use crate::model::{Component, ComponentStatus, Subtask, SubtaskType};
    use chrono::Utc;

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

    #[test]
    fn export_basic_contest() {
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

        let out = export(&dir, None).unwrap();
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

        let out = export(&dir, None).unwrap();
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
        let dir = std::env::temp_dir().join(format!("prep_lemon_inter_{}", uuid::Uuid::new_v4()));
        project::init_contest(&dir, "inter_test").unwrap();
        project::add_problem(
            &dir,
            project::NewProblem {
                id: "c",
                name: Some("C"),
                problem_type: Some(ProblemType::InteractiveLib),
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
        std::fs::write(pdir.join("auxiliary").join(format!("c.h")), "#pragma once").unwrap();

        let out = export(&dir, None).unwrap();
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
}
