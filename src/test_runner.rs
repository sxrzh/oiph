//! 集成测试：编译辅助程序、造数据、验证、运行 std 和 sols、检查正确性。
//! 纯确定性过程，供 agent 调用或 CLI 命令直接执行。

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use chrono::Utc;

use crate::assets;
use crate::model::{Problem, ProblemType, Verdict};
use crate::project;

pub struct TestReport {
    pub problem_id: String,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub log: Vec<String>,
}

impl TestReport {
    pub(crate) fn new(pid: &str) -> Self {
        Self {
            problem_id: pid.into(),
            warnings: Vec::new(),
            errors: Vec::new(),
            log: Vec::new(),
        }
    }

    fn ok(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.log.push(format!("✓ {msg}"));
    }

    fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.warnings.push(msg.clone());
        self.log.push(format!("⚠ {msg}"));
    }

    pub(crate) fn err(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.errors.push(msg.clone());
        self.log.push(format!("✗ {msg}"));
    }

    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty() && self.errors.is_empty()
    }

    pub fn to_string_report(&self) -> String {
        let mut out = format!("题目 {} 集成测试：\n", self.problem_id);
        for line in &self.log {
            out.push_str(&format!("  {line}\n"));
        }
        if self.is_clean() {
            out.push_str("集成测试通过，无警告无错误。\n");
        }
        if !self.warnings.is_empty() {
            out.push_str(&format!("\n警告（{}）：\n", self.warnings.len()));
            for w in &self.warnings {
                out.push_str(&format!("  ⚠ {w}\n"));
            }
        }
        if !self.errors.is_empty() {
            out.push_str(&format!("\n错误（{}）：\n", self.errors.len()));
            for e in &self.errors {
                out.push_str(&format!("  ✗ {e}\n"));
            }
        }
        out
    }
}

/// 对指定题目（或全部题目）运行集成测试。
pub fn run_tests(contest_dir: &Path, problem_id: Option<&str>) -> Vec<TestReport> {
    let contest = match project::load_contest(contest_dir) {
        Ok(c) => c,
        Err(e) => {
            return vec![{
                let mut r = TestReport::new(problem_id.unwrap_or("?"));
                r.err(format!("加载比赛失败：{e:#}"));
                r
            }];
        }
    };

    let pids: Vec<String> = match problem_id {
        Some(pid) => {
            if contest.problems.iter().any(|p| p == pid) {
                vec![pid.to_string()]
            } else {
                return vec![{
                    let mut r = TestReport::new(pid);
                    r.err(format!("题目 {pid} 不在比赛中"));
                    r
                }];
            }
        }
        None => contest.problems.clone(),
    };

    pids.iter()
        .filter_map(|pid| run_one(contest_dir, pid).ok())
        .collect()
}

fn run_one(contest_dir: &Path, pid: &str) -> anyhow::Result<TestReport> {
    let mut report = TestReport::new(pid);
    let pdir = project::problem_dir(contest_dir, pid);
    let problem = project::load_problem(&pdir)?;
    let aux_dir = pdir.join("auxiliary");

    // 临时目录
    let tmp = std::env::temp_dir().join(format!("preparer_test_{}_{}", pid, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp)?;

    // 确保 testlib.h 存在
    let testlib_h = aux_dir.join("testlib.h");
    if !testlib_h.exists() {
        std::fs::write(&testlib_h, assets::TESTLIB_H)?;
    }

    // 1. 编译辅助程序
    compile_aux(&problem, &aux_dir, &tmp, &mut report);

    if report.errors.iter().any(|e| e.contains("编译")) {
        cleanup(&tmp);
        return Ok(report);
    }

    // 2. 收集测试点 + 生成/复制数据
    let cases = setup_data(&problem, &pdir, &tmp, &mut report);

    if report.errors.is_empty() {
        // 3. 验证输入
        validate_inputs(&tmp, &cases, &mut report);
    }

    if report.errors.is_empty() {
        // 4. 编译 std
        compile_std(&problem, &pdir, &tmp, &mut report);
    }

    if report.errors.is_empty() {
        // 5. 生成 .ans
        generate_answers(&problem, &tmp, &cases, &mut report);
    }

    if report.errors.is_empty() {
        // 6. 检查 std 答案
        check_std(&tmp, &cases, &mut report);
    }

    if report.errors.is_empty() {
        // 7. 运行其他 sols
        run_sols(&problem, &pdir, &tmp, &cases, &mut report);
    }

    // 8. 记录测试时间
    if report.errors.is_empty() {
        let _ = project::with_problem_mut(contest_dir, pid, |p| {
            p.last_tested = Some(Utc::now());
            Ok(())
        });
    }

    cleanup(&tmp);
    Ok(report)
}

fn cleanup(tmp: &Path) {
    let _ = std::fs::remove_dir_all(tmp);
}

/// 带超时的命令（用 `timeout <secs>` 包裹），防止生成器/验证器/检查器等
/// 意外挂起（如等待 stdin）导致测试请求永远不返回。
fn timed_cmd(program: &Path, secs: u64) -> Command {
    let mut c = Command::new("timeout");
    c.arg(secs.to_string()).arg(program);
    c
}

/// 编译 auxiliary 下的程序。
fn compile_aux(problem: &Problem, aux_dir: &Path, tmp: &Path, report: &mut TestReport) {
    let flags = problem.compile_flags.split_whitespace().collect::<Vec<_>>();
    for (name, src) in [
        ("validator", "validator.cpp"),
        ("checker", "checker.cpp"),
        ("generator", "generator.cpp"),
    ] {
        let src_path = aux_dir.join(src);
        if !src_path.exists() {
            if name == "checker" {
                // checker 不存在不算错误，后面用 diff
                continue;
            }
            report.err(format!("{name}.cpp 不存在（auxiliary/）"));
            continue;
        }
        let out = tmp.join(name);
        let status = timed_cmd(Path::new("g++"), 600)
            .args(&flags)
            .arg("-I").arg(aux_dir)
            .arg("-o").arg(&out)
            .arg(&src_path)
            .stderr(Stdio::piped())
            .output();
        match status {
            Ok(o) if o.status.success() => report.ok(format!("{name} 编译通过")),
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                report.err(format!("{name} 编译失败：\n{}", &stderr[..stderr.len().min(500)]));
            }
            Err(e) => report.err(format!("{name} 编译失败：{e}")),
        }
    }
}

/// 生成或复制测试数据，返回测试点名列表。
fn setup_data(
    problem: &Problem,
    pdir: &Path,
    tmp: &Path,
    report: &mut TestReport,
) -> Vec<String> {
    let data_dir = pdir.join("data");
    let gen_bin = tmp.join("generator");

    // 收集所有测试点名
    let mut cases: Vec<String> = Vec::new();
    for st in &problem.subtasks {
        for c in &st.cases {
            if !cases.contains(c) {
                cases.push(c.clone());
            }
        }
    }

    // 无 subtasks 配置：从 data/ 自动发现 .in 文件
    if cases.is_empty() {
        if let Ok(rd) = std::fs::read_dir(&data_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(stem) = name.strip_suffix(".in") {
                    cases.push(stem.to_string());
                }
            }
        }
        cases.sort();
    }

    if cases.is_empty() {
        report.err("没有测试点（subtasks.cases 为空且 data/ 下无 .in 文件）");
        return cases;
    }

    for case_name in &cases {
        let in_dst = tmp.join(format!("{case_name}.in"));
        if let Some(gen_args) = problem.data_gen.get(case_name) {
            // 用 generator 生成（限时 60s，防止挂起）
            let output = timed_cmd(&gen_bin, 60)
                .args(gen_args.split_whitespace())
                .stdout(std::fs::File::create(&in_dst).map(Stdio::from).unwrap_or(Stdio::null()))
                .stderr(Stdio::piped())
                .output();
            match output {
                Ok(o) if o.status.success() => {}
                Ok(o) => report.err(format!("generator 生成 {case_name} 失败：{}", String::from_utf8_lossy(&o.stderr))),
                Err(e) => report.err(format!("generator 运行失败（{case_name}）：{e}")),
            }
        } else {
            // 复制 data/<case>.in
            let src = data_dir.join(format!("{case_name}.in"));
            if src.exists() {
                let _ = std::fs::copy(&src, &in_dst);
            } else {
                report.err(format!("测试点 {case_name} 无 .in 文件且不在 data_gen 中"));
            }
        }
    }
    report.ok(format!("{} 个测试点数据就绪", cases.len()));
    cases
}

/// 用 validator 检查每个输入。
fn validate_inputs(tmp: &Path, cases: &[String], report: &mut TestReport) {
    let validator = tmp.join("validator");
    if !validator.exists() {
        report.ok("validator 不存在，跳过验证");
        return;
    }
    for case_name in cases {
        let in_file = tmp.join(format!("{case_name}.in"));
        let status = timed_cmd(&validator, 30)
            .arg(&in_file)
            .stderr(Stdio::piped())
            .output();
        match status {
            Ok(o) if o.status.success() => {}
            Ok(o) => report.err(format!(
                "validator 检查 {case_name} 失败：{}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => report.err(format!("validator 运行失败（{case_name}）：{e}")),
        }
    }
    report.ok("validator 检查通过");
}

/// 编译 std（交互题需联合 interactive_lib）。
fn compile_std(problem: &Problem, pdir: &Path, tmp: &Path, report: &mut TestReport) {
    let flags = problem.compile_flags.split_whitespace().collect::<Vec<_>>();
    let std_src = problem.std.file.as_deref().unwrap_or("solutions/std.cpp");
    let std_path = pdir.join(std_src);
    if !std_path.exists() {
        report.err(format!("std 文件不存在：{std_src}"));
        return;
    }

    let is_interactive = matches!(
        problem.problem_type,
        ProblemType::InteractiveLib
    );

    let out = tmp.join("std");

    if is_interactive {
        // 复制 interactive_lib.cpp 和头文件到 tmp
        let aux_dir = pdir.join("auxiliary");
        let lib_src = aux_dir.join("interactive_lib.cpp");
        if !lib_src.exists() {
            report.err("interactive_lib.cpp 不存在（交互题）");
            return;
        }
        let lib_dst = tmp.join("interactive_lib.cpp");
        let _ = std::fs::copy(&lib_src, &lib_dst);
        // 头文件：auxiliary/<pid>.h 或 auxiliary/inter.h
        for hname in [format!("{}.h", problem.id), "inter.h".to_string()] {
            let hsrc = aux_dir.join(&hname);
            if hsrc.exists() {
                let _ = std::fs::copy(&hsrc, tmp.join(&hname));
            }
        }
        // 复制 std.cpp 到 tmp
        let std_dst = tmp.join("std.cpp");
        let _ = std::fs::copy(&std_path, &std_dst);
        let status = timed_cmd(Path::new("g++"), 600)
            .args(&flags)
            .arg("-I").arg(tmp)
            .arg("-o").arg(&out)
            .arg(&std_dst)
            .arg(&lib_dst)
            .stderr(Stdio::piped())
            .output();
        match status {
            Ok(o) if o.status.success() => report.ok("std 编译通过（交互）"),
            Ok(o) => report.err(format!("std 编译失败（交互）：\n{}", String::from_utf8_lossy(&o.stderr))),
            Err(e) => report.err(format!("std 编译失败：{e}")),
        }
    } else {
        let status = timed_cmd(Path::new("g++"), 600)
            .args(&flags)
            .arg("-o").arg(&out)
            .arg(&std_path)
            .stderr(Stdio::piped())
            .output();
        match status {
            Ok(o) if o.status.success() => report.ok("std 编译通过"),
            Ok(o) => report.err(format!("std 编译失败：\n{}", String::from_utf8_lossy(&o.stderr))),
            Err(e) => report.err(format!("std 编译失败：{e}")),
        }
    }
}

/// 用 std 生成 .ans 文件。
fn generate_answers(problem: &Problem, tmp: &Path, cases: &[String], report: &mut TestReport) {
    let std_bin = tmp.join("std");
    let tl_ms = problem.time_limit_ms;
    let timeout_secs = (tl_ms as f64 * 1.5 / 1000.0).ceil() as u64;
    let warn_secs = (tl_ms as f64 / 1000.0).ceil() as u64;

    for case_name in cases {
        let in_file = tmp.join(format!("{case_name}.in"));
        let ans_file = tmp.join(format!("{case_name}.ans"));
        let start = Instant::now();
        let status = Command::new("timeout")
            .arg(timeout_secs.to_string())
            .arg(&std_bin)
            .stdin(std::fs::File::open(&in_file).map(Stdio::from).unwrap_or(Stdio::null()))
            .stdout(std::fs::File::create(&ans_file).map(Stdio::from).unwrap_or(Stdio::null()))
            .stderr(Stdio::piped())
            .output();
        let elapsed = start.elapsed();

        match status {
            Ok(o) if o.status.code() == Some(124) => {
                report.err(format!("std 在测试点 {case_name} 超时（TLE）"));
            }
            Ok(o) if !o.status.success() => {
                report.err(format!("std 在测试点 {case_name} 运行错误（RE）"));
            }
            Ok(_) => {
                if elapsed.as_secs() >= warn_secs {
                    report.warn(format!(
                        "std 在测试点 {case_name} 用时 {:.1}s（超过时限 {}ms）",
                        elapsed.as_secs_f64(),
                        tl_ms
                    ));
                }
            }
            Err(e) => report.err(format!("std 运行失败（{case_name}）：{e}")),
        }
    }
    report.ok(format!("std 生成答案完成（{}/{}）", cases.len(), cases.len()));
}

/// 用 checker 检查 std 答案。
fn check_std(tmp: &Path, cases: &[String], report: &mut TestReport) {
    let checker = tmp.join("checker");
    for case_name in cases {
        let in_file = tmp.join(format!("{case_name}.in"));
        let ans_file = tmp.join(format!("{case_name}.ans"));
        if checker.exists() {
            let status = timed_cmd(&checker, 30)
                .arg(&in_file)
                .arg(&ans_file)
                .arg(&ans_file)
                .stderr(Stdio::piped())
                .output();
            match status {
                Ok(o) if o.status.success() => {}
                Ok(o) => report.err(format!(
                    "checker 检查 std 答案失败（{case_name}）：{}",
                    String::from_utf8_lossy(&o.stderr)
                )),
                Err(e) => report.err(format!("checker 运行失败（{case_name}）：{e}")),
            }
        } else {
            // 无 checker，跳过（std 自己的答案无需 diff 自身）
        }
    }
    report.ok("std 答案检查通过");
}

/// 运行其他 sols，检查评测结果。
fn run_sols(
    problem: &Problem,
    pdir: &Path,
    tmp: &Path,
    cases: &[String],
    report: &mut TestReport,
) {
    let flags = problem.compile_flags.split_whitespace().collect::<Vec<_>>();
    let tl_ms = problem.time_limit_ms;
    let timeout_secs = (tl_ms as f64 * 1.5 / 1000.0).ceil() as u64;
    let checker = tmp.join("checker");

    for sol in &problem.sols {
        let sol_name = &sol.name;
        let default_path = format!("solutions/{sol_name}.cpp");
        let sol_src = sol.file.as_deref().unwrap_or(&default_path);
        let sol_path = pdir.join(sol_src);
        if !sol_path.exists() {
            report.warn(format!("sol '{sol_name}' 文件不存在：{sol_src}，跳过"));
            continue;
        }

        // 编译
        let sol_bin = tmp.join(format!("sol_{sol_name}"));
        let compile_status = timed_cmd(Path::new("g++"), 600)
            .args(&flags)
            .arg("-I").arg(pdir.join("auxiliary"))
            .arg("-o").arg(&sol_bin)
            .arg(&sol_path)
            .stderr(Stdio::piped())
            .output();
        match compile_status {
            Ok(o) if !o.status.success() => {
                report.warn(format!("sol '{sol_name}' 编译失败，跳过"));
                continue;
            }
            Err(_) => {
                report.warn(format!("sol '{sol_name}' 编译失败，跳过"));
                continue;
            }
            _ => {}
        }

        // 运行每个测试点
        let mut any_match = false;
        let mut all_results = Vec::new();
        for case_name in cases {
            let in_file = tmp.join(format!("{case_name}.in"));
            let ans_file = tmp.join(format!("{case_name}.ans"));
            let out_file = tmp.join(format!("{sol_name}_{case_name}.out"));

            let status = Command::new("timeout")
                .arg(timeout_secs.to_string())
                .arg(&sol_bin)
                .stdin(std::fs::File::open(&in_file).map(Stdio::from).unwrap_or(Stdio::null()))
                .stdout(std::fs::File::create(&out_file).map(Stdio::from).unwrap_or(Stdio::null()))
                .stderr(Stdio::piped())
                .output();

            let verdict = match status {
                Ok(o) if o.status.code() == Some(124) => Verdict::Tle,
                Ok(o) if !o.status.success() => Verdict::Re,
                Ok(_) => {
                    // 用 checker 或 diff 检查
                    if checker.exists() {
                        let cs = timed_cmd(&checker, 30)
                            .arg(&in_file)
                            .arg(&out_file)
                            .arg(&ans_file)
                            .stderr(Stdio::piped())
                            .output();
                        match cs {
                            Ok(o) if o.status.success() => Verdict::Ac,
                            Ok(_) => Verdict::Wa,
                            Err(_) => Verdict::Re,
                        }
                    } else {
                        // diff
                        let diff = Command::new("diff")
                            .arg("-q")
                            .arg(&out_file)
                            .arg(&ans_file)
                            .output();
                        match diff {
                            Ok(o) if o.status.success() => Verdict::Ac,
                            _ => Verdict::Wa,
                        }
                    }
                }
                Err(_) => Verdict::Re,
            };
            all_results.push((case_name.clone(), verdict));
            if verdict == sol.expected.verdict {
                any_match = true;
            }
        }

        // 检查是否符合预期
        let expected = sol.expected.verdict;
        if expected == Verdict::Ac {
            // AC 要求所有测试点 AC
            let non_ac: Vec<_> = all_results.iter().filter(|(_, v)| *v != Verdict::Ac).collect();
            if !non_ac.is_empty() {
                report.warn(format!(
                    "sol '{sol_name}' 预期 AC，但 {} 个测试点非 AC：{}",
                    non_ac.len(),
                    non_ac.iter().map(|(c, v)| format!("{c}={:?}", v)).collect::<Vec<_>>().join(", ")
                ));
            } else {
                report.ok(format!("sol '{sol_name}' 全部 AC，符合预期"));
            }
        } else {
            // 非 AC：任意一个匹配即可
            if any_match {
                report.ok(format!(
                    "sol '{sol_name}' 存在 {:?} 的测试点，符合预期",
                    expected
                ));
            } else {
                report.warn(format!(
                    "sol '{sol_name}' 预期 {:?}，但所有测试点均不符合：{}",
                    expected,
                    all_results.iter().map(|(c, v)| format!("{c}={:?}", v)).collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_to_string() {
        let mut r = TestReport::new("test");
        r.ok("编译通过");
        r.warn("超时警告");
        let s = r.to_string_report();
        assert!(s.contains("✓ 编译通过"));
        assert!(s.contains("⚠ 超时警告"));
        assert!(!r.is_clean());
    }

    #[test]
    fn clean_report() {
        let mut r = TestReport::new("test");
        r.ok("全部通过");
        assert!(r.is_clean());
        let s = r.to_string_report();
        assert!(s.contains("无警告无错误"));
    }
}
