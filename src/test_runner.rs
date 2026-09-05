//! 集成测试：编译辅助程序、造数据、验证、运行 std 和 sols、检查正确性。
//! 纯确定性过程，供 agent 调用或 CLI 命令直接执行。

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use chrono::Utc;

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
    let tmp = std::env::temp_dir().join(format!("oiph_test_{}_{}", pid, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp)?;

    // 确保 testlib.h 存在（从 ~/.oiph/vendor/testlib.h 复制；启动时已检查 vendor 存在）
    let testlib_h = aux_dir.join("testlib.h");
    if !testlib_h.exists()
        && let Err(e) = crate::paths::vendor_read("testlib.h")
            .and_then(|c| std::fs::write(&testlib_h, c.as_bytes()).map_err(|e| e.into()))
    {
        report.err(format!("写入 testlib.h 失败：{e:#}"));
        return Ok(report);
    }

    // 1. 编译辅助程序
    compile_aux(&problem, &aux_dir, &tmp, &mut report);

    if !report.errors.is_empty() {
        cleanup(&tmp);
        return Ok(report);
    }

    // 2. 收集测试点 + 生成/复制数据
    let cases = setup_data(&problem, &pdir, &tmp, &mut report);

    // 没有任何测试数据：只给警告，跳过后续步骤
    if cases.is_empty() {
        cleanup(&tmp);
        return Ok(report);
    }

    let is_answer_only = problem.problem_type == ProblemType::AnswerOnly;

    if report.errors.is_empty() {
        // 3. 验证输入
        validate_inputs(&tmp, &cases, &mut report);
    }

    if is_answer_only {
        report.ok("提交答案题：无 std，跳过编译与答案生成");
    } else if report.errors.is_empty() {
        // 4. 编译 std
        compile_std(&problem, &pdir, &tmp, &mut report);
    }

    if !is_answer_only && report.errors.is_empty() {
        // 5. 生成 .ans
        generate_answers(&problem, &tmp, &cases, &mut report);
    }

    if !is_answer_only && report.errors.is_empty() {
        // 6. 检查 std 答案
        check_std(&tmp, &cases, &mut report);
    }

    if report.errors.is_empty() {
        // 7. 运行其他 sols
        if is_answer_only {
            run_answer_only_sols(&problem, &pdir, &tmp, &cases, &mut report);
        } else {
            run_sols(&problem, &pdir, &tmp, &cases, &mut report);
        }
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
        report.warn("没有任何测试数据（subtasks.cases 为空且 data/ 下无 .in 文件）");
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
        // testlib validator 默认从 stdin 读取输入，用重定向而不是命令行参数传递
        let status = timed_cmd(&validator, 30)
            .stdin(std::fs::File::open(&in_file).map(Stdio::from).unwrap_or(Stdio::null()))
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

/// 编译 std：
/// - 函数交互题（interactive_lib / function）：std.cpp 与 auxiliary/interactive_lib.cpp
///   及头文件放同一目录联合编译，运行与传统题相同；
/// - IO 交互题（interactive_io）：std 单独编译，interactive_lib.cpp 单独编译为 grader，
///   运行时用 fifo 相连；
/// - 提交答案题：无 std，跳过。
fn compile_std(problem: &Problem, pdir: &Path, tmp: &Path, report: &mut TestReport) {
    let flags = problem.compile_flags.split_whitespace().collect::<Vec<_>>();
    let std_src = problem.std.file.as_deref().unwrap_or("solutions/std.cpp");
    let std_path = pdir.join(std_src);
    let aux_dir = pdir.join("auxiliary");
    let out = tmp.join("std");

    match problem.problem_type {
        ProblemType::AnswerOnly => {
            report.ok("提交答案题：无 std，跳过编译");
        }
        ProblemType::Function => {
            if !std_path.exists() {
                report.err(format!("std 文件不存在：{std_src}"));
                return;
            }
            compile_with_interactive_lib(
                &flags,
                &std_path,
                "std.cpp",
                &out,
                &aux_dir,
                tmp,
                "std",
                report,
            );
        }
        ProblemType::InteractiveIO => {
            if !std_path.exists() {
                report.err(format!("std 文件不存在：{std_src}"));
                return;
            }
            // std 与传统题相同单独编译
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
            // interactive_lib.cpp 单独编译为 grader
            compile_grader(&flags, &aux_dir, tmp, report);
        }
        ProblemType::Traditional => {
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
}

/// 复制 interactive_lib.cpp 与 auxiliary 下全部头文件到 tmp。
/// interactive_lib.cpp 不存在时返回 false。
fn copy_interactive_sources(aux_dir: &Path, tmp: &Path) -> bool {
    let lib_src = aux_dir.join("interactive_lib.cpp");
    if !lib_src.exists() {
        return false;
    }
    let _ = std::fs::copy(&lib_src, tmp.join("interactive_lib.cpp"));
    if let Ok(rd) = std::fs::read_dir(aux_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".h") {
                let _ = std::fs::copy(e.path(), tmp.join(&name));
            }
        }
    }
    true
}

/// 函数交互/函数题联合编译：`g++ <flags> xxx.cpp interactive_lib.cpp`，
/// 源文件、头文件与 interactive_lib.cpp 置于同一目录（tmp）。
#[allow(clippy::too_many_arguments)]
fn compile_with_interactive_lib(
    flags: &[&str],
    src_path: &Path,
    src_copy_name: &str,
    out: &Path,
    aux_dir: &Path,
    tmp: &Path,
    label: &str,
    report: &mut TestReport,
) -> bool {
    if !copy_interactive_sources(aux_dir, tmp) {
        report.err("interactive_lib.cpp 不存在（函数交互题）");
        return false;
    }
    let src_dst = tmp.join(src_copy_name);
    let _ = std::fs::copy(src_path, &src_dst);
    let status = timed_cmd(Path::new("g++"), 600)
        .args(flags)
        .arg("-I").arg(tmp)
        .arg("-o").arg(out)
        .arg(&src_dst)
        .arg(tmp.join("interactive_lib.cpp"))
        .stderr(Stdio::piped())
        .output();
    match status {
        Ok(o) if o.status.success() => {
            report.ok(format!("{label} 编译通过（联合 interactive_lib）"));
            true
        }
        Ok(o) => {
            report.err(format!("{label} 编译失败：\n{}", String::from_utf8_lossy(&o.stderr)));
            false
        }
        Err(e) => {
            report.err(format!("{label} 编译失败：{e}"));
            false
        }
    }
}

/// IO 交互题：interactive_lib.cpp 单独编译为 grader。
fn compile_grader(flags: &[&str], aux_dir: &Path, tmp: &Path, report: &mut TestReport) {
    let lib_src = aux_dir.join("interactive_lib.cpp");
    if !lib_src.exists() {
        report.err("interactive_lib.cpp 不存在（IO 交互题）");
        return;
    }
    let out = tmp.join("grader");
    let status = timed_cmd(Path::new("g++"), 600)
        .args(flags)
        .arg("-I").arg(aux_dir)
        .arg("-o").arg(&out)
        .arg(&lib_src)
        .stderr(Stdio::piped())
        .output();
    match status {
        Ok(o) if o.status.success() => report.ok("grader（interactive_lib）编译通过"),
        Ok(o) => report.err(format!("grader 编译失败：\n{}", String::from_utf8_lossy(&o.stderr))),
        Err(e) => report.err(format!("grader 编译失败：{e}")),
    }
}

/// IO 交互题单测试点运行脚本：mkfifo 建立待测程序与 grader 的双向管道。
/// grader 的退出码写入 grader_rc 文件（PIPESTATUS[0]），供调用方检查。
fn io_interactive_script(participant: &str, in_name: &str, out_name: &str, timeout_secs: u64) -> String {
    format!(
        "mkfifo pipe_in pipe_out\n\
         timeout {t} ./{p} < pipe_in | tee pipe_out &\n\
         timeout {t} ./grader \"{i}\" \"{o}\" < pipe_out | tee pipe_in\n\
         echo ${{PIPESTATUS[0]}} > grader_rc\n\
         wait\n\
         rm -f pipe_in pipe_out\n",
        t = timeout_secs,
        p = participant,
        i = in_name,
        o = out_name,
    )
}

/// 运行 IO 交互脚本并返回 (整体 status, grader 退出码)。
fn run_io_script(tmp: &Path, script: &str, timeout_secs: u64) -> (Option<i32>, Option<i32>) {
    let status = Command::new("timeout")
        .arg(timeout_secs.to_string())
        .arg("bash")
        .arg("-c")
        .arg(script)
        .current_dir(tmp)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let rc_file = tmp.join("grader_rc");
    let grader_rc = std::fs::read_to_string(&rc_file)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok());
    let _ = std::fs::remove_file(&rc_file);
    let code = match &status {
        Ok(o) => o.status.code(),
        Err(_) => None,
    };
    (code, grader_rc)
}

/// 用 std 生成 .ans 文件。
/// IO 交互题：std 与 grader 通过 fifo 相连，grader 写出答案文件；
/// 其余类型：std 独立运行。
fn generate_answers(problem: &Problem, tmp: &Path, cases: &[String], report: &mut TestReport) {
    let std_bin = tmp.join("std");
    let tl_ms = problem.time_limit_ms;
    let timeout_secs = (tl_ms as f64 * 1.5 / 1000.0).ceil() as u64;
    let warn_secs = (tl_ms as f64 / 1000.0).ceil() as u64;

    if problem.problem_type == ProblemType::InteractiveIO {
        let grader = tmp.join("grader");
        if !grader.exists() {
            report.err("grader 不存在（IO 交互题）");
            return;
        }
        for case_name in cases {
            let ans_file = tmp.join(format!("{case_name}.ans"));
            let _ = std::fs::remove_file(&ans_file);
            let script = io_interactive_script(
                "std",
                &format!("{case_name}.in"),
                &format!("{case_name}.ans"),
                timeout_secs,
            );
            let start = Instant::now();
            let (code, grader_rc) = run_io_script(tmp, &script, timeout_secs);
            let elapsed = start.elapsed();

            match code {
                Some(124) => {
                    report.err(format!("std 在测试点 {case_name} 超时（TLE）"));
                }
                Some(c) if c != 0 => {
                    report.err(format!("std 在测试点 {case_name} 运行错误（RE）"));
                }
                _ => {
                    // grader 返回值必须为 0，否则记为 RE
                    if grader_rc != Some(0) {
                        report.err(format!(
                            "grader 在测试点 {case_name} 异常退出（rc={:?}，RE）",
                            grader_rc
                        ));
                    } else if elapsed.as_secs() >= warn_secs {
                        report.warn(format!(
                            "std 在测试点 {case_name} 用时 {:.1}s（超过时限 {}ms）",
                            elapsed.as_secs_f64(),
                            tl_ms
                        ));
                    }
                }
            }
        }
        report.ok(format!("std（grader）生成答案完成（{}/{}）", cases.len(), cases.len()));
        return;
    }

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
    if !checker.exists() {
        // compile_aux 已要求 checker 存在，防御性报错
        report.err("checker 不存在");
        return;
    }
    for case_name in cases {
        let in_file = tmp.join(format!("{case_name}.in"));
        let ans_file = tmp.join(format!("{case_name}.ans"));
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
    }
    report.ok("std 答案检查通过");
}

/// 用 checker 判定输出文件。checker 缺失时由 compile_aux 报错并中止，
/// 此处防御性记 RE。
fn judge_output(checker: &Path, in_file: &Path, out_file: &Path, ans_file: &Path) -> Verdict {
    if !checker.exists() {
        return Verdict::Re;
    }
    let cs = timed_cmd(checker, 30)
        .arg(in_file)
        .arg(out_file)
        .arg(ans_file)
        .stderr(Stdio::piped())
        .output();
    match cs {
        Ok(o) if o.status.success() => Verdict::Ac,
        Ok(_) => Verdict::Wa,
        Err(_) => Verdict::Re,
    }
}

/// 汇总 sol 的测试点结果并与预期比对。
fn check_sol_expectation(
    report: &mut TestReport,
    sol_name: &str,
    expected: Verdict,
    all_results: &[(String, Verdict)],
) {
    if expected == Verdict::Ac {
        // AC 要求所有测试点 AC
        let non_ac: Vec<_> = all_results.iter().filter(|(_, v)| *v != Verdict::Ac).collect();
        if !non_ac.is_empty() {
            report.warn(format!(
                "sol '{sol_name}' 预期 AC，但 {} 个测试点非 AC：{}",
                non_ac.len(),
                non_ac.iter().map(|(c, v)| format!("{c}={v:?}")).collect::<Vec<_>>().join(", ")
            ));
        } else {
            report.ok(format!("sol '{sol_name}' 全部 AC，符合预期"));
        }
    } else {
        // 非 AC：任意一个匹配即可
        let any_match = all_results.iter().any(|(_, v)| *v == expected);
        if any_match {
            report.ok(format!(
                "sol '{sol_name}' 存在 {expected:?} 的测试点，符合预期"
            ));
        } else {
            report.warn(format!(
                "sol '{sol_name}' 预期 {expected:?}，但所有测试点均不符合：{}",
                all_results.iter().map(|(c, v)| format!("{c}={v:?}")).collect::<Vec<_>>().join(", ")
            ));
        }
    }
}

/// 运行其他 sols，检查评测结果。
/// - 函数交互题（interactive_lib / function）：sol 与 interactive_lib.cpp 联合编译，运行与传统题相同；
/// - IO 交互题（interactive_io）：sol 单独编译，与 grader 通过 fifo 相连运行；
/// - 传统题：单独编译运行。
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
    let aux_dir = pdir.join("auxiliary");
    let io_interactive = problem.problem_type == ProblemType::InteractiveIO;
    let with_lib = problem.problem_type == ProblemType::Function;

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
        let compile_ok = if with_lib {
            compile_with_interactive_lib(
                &flags,
                &sol_path,
                &format!("sol_{sol_name}.cpp"),
                &sol_bin,
                &aux_dir,
                tmp,
                &format!("sol '{sol_name}'"),
                report,
            )
        } else {
            let compile_status = timed_cmd(Path::new("g++"), 600)
                .args(&flags)
                .arg("-I").arg(&aux_dir)
                .arg("-o").arg(&sol_bin)
                .arg(&sol_path)
                .stderr(Stdio::piped())
                .output();
            match compile_status {
                Ok(o) if o.status.success() => true,
                Ok(o) => {
                    report.warn(format!(
                        "sol '{sol_name}' 编译失败，跳过：{}",
                        String::from_utf8_lossy(&o.stderr)
                    ));
                    false
                }
                Err(_) => {
                    report.warn(format!("sol '{sol_name}' 编译失败，跳过"));
                    false
                }
            }
        };
        if !compile_ok {
            continue;
        }

        // 运行每个测试点
        let mut all_results = Vec::new();
        for case_name in cases {
            let in_file = tmp.join(format!("{case_name}.in"));
            let ans_file = tmp.join(format!("{case_name}.ans"));
            let out_file = tmp.join(format!("{sol_name}_{case_name}.out"));

            let verdict = if io_interactive {
                let script = io_interactive_script(
                    &format!("sol_{sol_name}"),
                    &format!("{case_name}.in"),
                    &format!("{sol_name}_{case_name}.out"),
                    timeout_secs,
                );
                let _ = std::fs::remove_file(&out_file);
                let (code, grader_rc) = run_io_script(tmp, &script, timeout_secs);
                match code {
                    Some(124) => Verdict::Tle,
                    Some(c) if c != 0 => Verdict::Re,
                    // grader 返回值非 0 → RE
                    _ if grader_rc != Some(0) => Verdict::Re,
                    _ => judge_output(&checker, &in_file, &out_file, &ans_file),
                }
            } else {
                let status = Command::new("timeout")
                    .arg(timeout_secs.to_string())
                    .arg(&sol_bin)
                    .stdin(std::fs::File::open(&in_file).map(Stdio::from).unwrap_or(Stdio::null()))
                    .stdout(std::fs::File::create(&out_file).map(Stdio::from).unwrap_or(Stdio::null()))
                    .stderr(Stdio::piped())
                    .output();

                match status {
                    Ok(o) if o.status.code() == Some(124) => Verdict::Tle,
                    Ok(o) if !o.status.success() => Verdict::Re,
                    Ok(_) => judge_output(&checker, &in_file, &out_file, &ans_file),
                    Err(_) => Verdict::Re,
                }
            };
            all_results.push((case_name.clone(), verdict));
        }

        check_sol_expectation(report, sol_name, sol.expected.verdict, &all_results);
    }
}

/// 提交答案题：没有 std，sols 都是目录（内含各测试点的输出文件），
/// 直接用 checker（或 diff）与 data/ 中的标准输出对比。
fn run_answer_only_sols(
    problem: &Problem,
    pdir: &Path,
    tmp: &Path,
    cases: &[String],
    report: &mut TestReport,
) {
    let data_dir = pdir.join("data");
    let checker = tmp.join("checker");

    // 标准答案必须齐备
    let missing: Vec<&String> = cases
        .iter()
        .filter(|c| !data_dir.join(format!("{c}.ans")).exists())
        .collect();
    if !missing.is_empty() {
        report.err(format!(
            "data/ 缺少标准输出（.ans）：{}",
            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
        return;
    }

    for sol in &problem.sols {
        let sol_name = &sol.name;
        let default_dir = format!("solutions/{}", sol.name);
        let sol_dir = pdir.join(sol.file.as_deref().unwrap_or(&default_dir));
        if !sol_dir.is_dir() {
            report.warn(format!(
                "sol '{sol_name}' 目录不存在：{}，跳过",
                sol_dir.display()
            ));
            continue;
        }
        let mut all_results = Vec::new();
        for case_name in cases {
            let in_file = tmp.join(format!("{case_name}.in"));
            let ans_file = data_dir.join(format!("{case_name}.ans"));
            // sol 目录下的输出文件：{case}.out 优先，{case}.ans 兜底
            let out_file = [format!("{case_name}.out"), format!("{case_name}.ans")]
                .iter()
                .map(|n| sol_dir.join(n))
                .find(|p| p.exists());
            let verdict = match out_file {
                Some(out) => judge_output(&checker, &in_file, &out, &ans_file),
                // 缺少该测试点的输出
                None => Verdict::Wa,
            };
            all_results.push((case_name.clone(), verdict));
        }
        check_sol_expectation(report, sol_name, sol.expected.verdict, &all_results);
    }
    report.ok("提交答案题 sols 检查完成");
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
