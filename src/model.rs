//! 数据模型：比赛、题目、各组件状态，以及 `GetStatus` trait。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 枚举类型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ProblemType {
    #[default]
    Traditional,
    InteractiveLib,
    InteractiveIO,
    AnswerOnly,
    Function,
}


impl ProblemType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Traditional => "传统题",
            Self::InteractiveLib => "函数交互",
            Self::InteractiveIO => "IO 交互",
            Self::AnswerOnly => "提交答案",
            Self::Function => "函数题",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ProblemSource {
    #[default]
    Original,
    Moved,
    Adapted,
}


impl ProblemSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Original => "原创",
            Self::Moved => "搬运",
            Self::Adapted => "改编",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DataType {
    Blob,
    #[default]
    Generated,
}


impl DataType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Blob => "现成数据",
            Self::Generated => "生成数据",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Verdict {
    Ac,
    Wa,
    Tle,
    Mle,
    Re,
    Partial,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ac => "AC",
            Self::Wa => "WA",
            Self::Tle => "TLE",
            Self::Mle => "MLE",
            Self::Re => "RE",
            Self::Partial => "PARTIAL",
        }
    }

    /// 从字符串宽松解析：接受大小写、中文等变体。
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_lowercase();
        Some(match s.as_str() {
            "ac" | "accepted" | "通过" => Self::Ac,
            "wa" | "wrong answer" | "答案错误" => Self::Wa,
            "tle" | "time limit" | "超时" => Self::Tle,
            "mle" | "memory limit" | "超内存" => Self::Mle,
            "re" | "runtime error" | "运行错误" => Self::Re,
            "partial" | "部分分" => Self::Partial,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// 组件状态
// ---------------------------------------------------------------------------

/// 组件状态。使用内部标签序列化为 YAML：
/// `{ state: not_started }` / `{ state: in_progress, progress: 0.3, message: "..." }`
/// / `{ state: completed, timestamp: "..." }` / `{ state: failed, error: "..." }`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[derive(Default)]
pub enum ComponentStatus {
    #[default]
    NotStarted,
    InProgress {
        #[serde(default)]
        progress: f32,
        #[serde(default)]
        message: String,
    },
    Completed {
        timestamp: DateTime<Utc>,
    },
    Failed {
        error: String,
    },
}


impl ComponentStatus {
    pub fn completed_now() -> Self {
        Self::Completed {
            timestamp: Utc::now(),
        }
    }

    pub fn in_progress(progress: f32, message: impl Into<String>) -> Self {
        Self::InProgress {
            progress: progress.clamp(0.0, 1.0),
            message: message.into(),
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self::Failed {
            error: error.into(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::NotStarted => "未开始".to_string(),
            Self::InProgress { progress, message } => {
                if message.is_empty() {
                    format!("进行中（{}%）", (progress * 100.0).round() as u32)
                } else {
                    format!(
                        "进行中（{}%）- {}",
                        (progress * 100.0).round() as u32,
                        message
                    )
                }
            }
            Self::Completed { timestamp } => {
                format!("已完成 @ {}", timestamp.format("%Y-%m-%d %H:%M:%S"))
            }
            Self::Failed { error } => format!("失败：{error}"),
        }
    }

    /// 是否已成功完成（测试与状态判断用）。
    #[allow(dead_code)]
    pub fn is_terminal_ok(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }

    /// 是否失败。
    #[allow(dead_code)]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// 聚合多个组件状态：Failed > InProgress > Completed > NotStarted。
pub fn aggregate(statuses: Vec<ComponentStatus>) -> ComponentStatus {
    if statuses.is_empty() {
        return ComponentStatus::NotStarted;
    }
    let mut failed = Vec::new();
    let mut in_prog = Vec::new();
    let mut completed = Vec::new();
    for s in &statuses {
        match s {
            ComponentStatus::Failed { error } => failed.push(error.clone()),
            ComponentStatus::InProgress { progress, message } => {
                in_prog.push((*progress, message.clone()));
            }
            ComponentStatus::Completed { .. } => completed.push(()),
            ComponentStatus::NotStarted => {}
        }
    }
    if !failed.is_empty() {
        let error = if failed.len() == 1 {
            failed.remove(0)
        } else {
            format!("{} 个组件失败：{}", failed.len(), failed.join("；"))
        };
        return ComponentStatus::failed(error);
    }
    if !in_prog.is_empty() {
        let avg = in_prog.iter().map(|(p, _)| *p).sum::<f32>() / in_prog.len() as f32;
        let names = in_prog
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(_, m)| m.as_str())
            .collect::<Vec<_>>()
            .join("、");
        return ComponentStatus::in_progress(
            avg,
            if names.is_empty() {
                "进行中".to_string()
            } else {
                names
            },
        );
    }
    if completed.len() == statuses.len() {
        // 取最大时间戳
        let ts = statuses
            .iter()
            .filter_map(|s| match s {
                ComponentStatus::Completed { timestamp } => Some(*timestamp),
                _ => None,
            })
            .max()
            .unwrap_or_else(Utc::now);
        return ComponentStatus::Completed { timestamp: ts };
    }
    ComponentStatus::NotStarted
}

pub trait GetStatus {
    fn get_status(&self) -> ComponentStatus;
}

impl GetStatus for ComponentStatus {
    fn get_status(&self) -> ComponentStatus {
        self.clone()
    }
}

// ---------------------------------------------------------------------------
// 组件结构
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Component {
    #[serde(default)]
    pub status: ComponentStatus,
}

impl Component {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgingStatus {
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

impl Default for JudgingStatus {
    fn default() -> Self {
        Self {
            verdict: Verdict::Ac,
            score: Some(100.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolutionStatus {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub expected: JudgingStatus,
    #[serde(default)]
    pub status: ComponentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStatus {
    #[serde(default)]
    pub data_type: DataType,
    #[serde(default)]
    pub status: ComponentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateCheckResult {
    pub found: bool,
    #[serde(default)]
    pub matches: Vec<String>,
    pub checked_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemFiles {
    #[serde(default = "ProblemFiles::def_statement")]
    pub statement: String,
    #[serde(default = "ProblemFiles::def_down_dir")]
    pub down_dir: String,
    #[serde(default = "ProblemFiles::def_data_dir")]
    pub data_dir: String,
    #[serde(default = "ProblemFiles::def_aux_dir")]
    pub aux_dir: String,
    #[serde(default = "ProblemFiles::def_solutions_dir")]
    pub solutions_dir: String,
    #[serde(default = "ProblemFiles::def_std_file")]
    pub std_file: Option<String>,
    #[serde(default)]
    pub tutorial: Option<String>,
}

impl Default for ProblemFiles {
    fn default() -> Self {
        Self {
            statement: Self::def_statement(),
            down_dir: Self::def_down_dir(),
            data_dir: Self::def_data_dir(),
            aux_dir: Self::def_aux_dir(),
            solutions_dir: Self::def_solutions_dir(),
            std_file: Self::def_std_file(),
            tutorial: None,
        }
    }
}

impl ProblemFiles {
    fn def_statement() -> String {
        "statement/zh_cn.md".into()
    }
    fn def_down_dir() -> String {
        "statement/down".into()
    }
    fn def_data_dir() -> String {
        "data".into()
    }
    fn def_aux_dir() -> String {
        "auxiliary".into()
    }
    fn def_solutions_dir() -> String {
        "solutions".into()
    }
    fn def_std_file() -> Option<String> {
        Some("solutions/std.cpp".into())
    }
}

// ---------------------------------------------------------------------------
// 题目
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Problem {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub problem_type: ProblemType,
    #[serde(default)]
    pub source: ProblemSource,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "Problem::def_time_limit")]
    pub time_limit_ms: u64,
    #[serde(default = "Problem::def_memory_limit")]
    pub memory_limit_mb: u64,
    #[serde(default = "Problem::def_compile_flags")]
    pub compile_flags: String,
    /// 测试点配置（subtasks 列表）。
    #[serde(default)]
    pub subtasks: Vec<Subtask>,
    /// 数据生成参数：key 为测试点名称（subtasks.cases 中的项），
    /// value 为 generator 命令行参数。生成时执行 `<generator> <value>`。
    #[serde(default)]
    pub data_gen: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub statement: ComponentStatus,
    #[serde(default = "Problem::def_std")]
    pub std: SolutionStatus,
    #[serde(default)]
    pub sols: Vec<SolutionStatus>,
    #[serde(default = "Problem::def_data")]
    pub data: DataStatus,
    #[serde(default)]
    pub validator: Component,
    #[serde(default)]
    pub checker: Component,
    #[serde(default)]
    pub interactive_lib: Option<Component>,
    #[serde(default)]
    pub tutorial: ComponentStatus,
    #[serde(default)]
    pub duplicate_check: Option<DuplicateCheckResult>,
    #[serde(default)]
    pub files: ProblemFiles,
}

impl Problem {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Default::default()
        }
    }

    fn def_time_limit() -> u64 {
        1000
    }
    fn def_memory_limit() -> u64 {
        512
    }
    fn def_compile_flags() -> String {
        "-O2 -std=c++14".into()
    }
    fn def_data() -> DataStatus {
        DataStatus {
            data_type: DataType::Generated,
            status: ComponentStatus::NotStarted,
        }
    }
    fn def_std() -> SolutionStatus {
        SolutionStatus {
            name: "std".into(),
            file: Some("solutions/std.cpp".into()),
            expected: JudgingStatus::default(),
            status: ComponentStatus::NotStarted,
        }
    }

    /// 题目目录内的所有组件状态（用于聚合）。
    pub fn component_statuses(&self) -> Vec<ComponentStatus> {        let mut v = vec![
            self.statement.clone(),
            self.std.status.clone(),
        ];
        for s in &self.sols {
            v.push(s.status.clone());
        }
        v.push(self.data.status.clone());
        v.push(self.validator.status.clone());
        v.push(self.checker.status.clone());
        v.push(self.tutorial.clone());
        if let Some(c) = &self.interactive_lib {
            v.push(c.status.clone());
        }
        v
    }
}

impl Default for Problem {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            problem_type: ProblemType::default(),
            source: ProblemSource::default(),
            tags: Vec::new(),
            time_limit_ms: Self::def_time_limit(),
            memory_limit_mb: Self::def_memory_limit(),
            compile_flags: Self::def_compile_flags(),
            subtasks: Vec::new(),
            data_gen: std::collections::BTreeMap::new(),
            statement: ComponentStatus::NotStarted,
            std: Self::def_std(),
            sols: Vec::new(),
            data: Self::def_data(),
            validator: Component::new(),
            checker: Component::new(),
            interactive_lib: None,
            tutorial: ComponentStatus::NotStarted,
            duplicate_check: None,
            files: ProblemFiles::default(),
        }
    }
}

impl GetStatus for Problem {
    fn get_status(&self) -> ComponentStatus {
        aggregate(self.component_statuses())
    }
}

impl GetStatus for SolutionStatus {
    fn get_status(&self) -> ComponentStatus {
        self.status.clone()
    }
}

impl GetStatus for DataStatus {
    fn get_status(&self) -> ComponentStatus {
        self.status.clone()
    }
}

impl GetStatus for Component {
    fn get_status(&self) -> ComponentStatus {
        self.status.clone()
    }
}

// ---------------------------------------------------------------------------
// 比赛
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContestConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_min: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// 比赛结构。
///
/// 序列化时 `problems` 字段保存的是题目目录名列表（写入 `config.yaml`）；
/// `loaded_problems` 在加载时由 [`crate::project::load_contest`] 从各题目目录填充，
/// 不参与序列化。内存中 [`Self::problems`] 聚合自 `loaded_problems`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contest {
    pub id: String,
    pub name: String,
    /// 比赛目录下的题目目录名列表（持久化字段）。
    #[serde(default)]
    pub problems: Vec<String>,
    #[serde(default)]
    pub config: ContestConfig,
    pub created_at: DateTime<Utc>,
    /// 加载后的题目列表（不序列化，由工程层填充）。
    #[serde(skip)]
    pub loaded_problems: Vec<Problem>,
}

impl Contest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            problems: Vec::new(),
            config: ContestConfig::default(),
            created_at: Utc::now(),
            loaded_problems: Vec::new(),
        }
    }
}

impl GetStatus for Contest {
    fn get_status(&self) -> ComponentStatus {
        if self.loaded_problems.is_empty() {
            if self.problems.is_empty() {
                return ComponentStatus::NotStarted;
            }
            // 题目尚未加载，无法判断，保守视为进行中。
            return ComponentStatus::in_progress(0.0, "题目尚未加载");
        }
        aggregate(
            self.loaded_problems
                .iter()
                .map(|p| p.get_status())
                .collect(),
        )
    }
}

// ---------------------------------------------------------------------------
// 测试点配置（放在题目 config.yaml 的 subtasks 字段中）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SubtaskType {
    #[default]
    Sum,
    Min,
    Mul,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub score: f64,
    #[serde(rename = "type")]
    pub stype: SubtaskType,
    #[serde(default)]
    pub cases: Vec<String>,
    #[serde(default)]
    pub pretest: bool,
    #[serde(default)]
    pub sample: bool,
    #[serde(default)]
    pub depend: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_failed_wins() {
        let s = aggregate(vec![
            ComponentStatus::completed_now(),
            ComponentStatus::failed("boom"),
            ComponentStatus::NotStarted,
        ]);
        assert!(s.is_failed());
    }

    #[test]
    fn aggregate_in_progress_wins_over_completed() {
        let s = aggregate(vec![
            ComponentStatus::completed_now(),
            ComponentStatus::in_progress(0.5, "x"),
        ]);
        assert!(matches!(s, ComponentStatus::InProgress { .. }));
    }

    #[test]
    fn aggregate_all_completed() {
        let s = aggregate(vec![ComponentStatus::completed_now(), ComponentStatus::completed_now()]);
        assert!(s.is_terminal_ok());
    }

    #[test]
    fn aggregate_empty_is_not_started() {
        assert!(matches!(aggregate(vec![]), ComponentStatus::NotStarted));
    }

    #[test]
    fn problem_yaml_roundtrip() {
        let mut p = Problem::new("p1");
        p.name = "A+B".into();
        p.tags = vec!["dp".into()];
        p.statement = ComponentStatus::completed_now();
        p.sols.push(SolutionStatus {
            name: "brute".into(),
            file: Some("solutions/brute.cpp".into()),
            expected: JudgingStatus {
                verdict: Verdict::Wa,
                score: Some(30.0),
            },
            status: ComponentStatus::completed_now(),
        });
        let yaml = serde_yaml::to_string(&p).unwrap();
        let q: Problem = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(q.id, "p1");
        assert_eq!(q.name, "A+B");
        assert_eq!(q.tags, vec!["dp"]);
        assert_eq!(q.sols.len(), 1);
        assert_eq!(q.sols[0].expected.verdict, Verdict::Wa);
        assert!(q.statement.is_terminal_ok());
    }

    #[test]
    fn contest_yaml_keeps_problem_ids_only() {
        let mut c = Contest::new("test");
        c.problems = vec!["a".into(), "b".into()];
        c.loaded_problems.push(Problem::new("a"));
        let yaml = serde_yaml::to_string(&c).unwrap();
        assert!(yaml.contains("problems:"));
        assert!(yaml.contains("- a"));
        assert!(!yaml.contains("loaded_problems"));
        let c2: Contest = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(c2.problems, vec!["a", "b"]);
        assert!(c2.loaded_problems.is_empty());
    }

    #[test]
    fn component_status_yaml_internal_tag() {
        let s = ComponentStatus::in_progress(0.3, "hi");
        let yaml = serde_yaml::to_string(&s).unwrap();
        assert!(yaml.contains("state: in_progress"));
        let back: ComponentStatus = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn verdict_parse_accepts_variants() {
        assert_eq!(Verdict::parse("AC"), Some(Verdict::Ac));
        assert_eq!(Verdict::parse("wa"), Some(Verdict::Wa));
        assert_eq!(Verdict::parse("答案错误"), Some(Verdict::Wa));
        assert_eq!(Verdict::parse("nope"), None);
    }
}
