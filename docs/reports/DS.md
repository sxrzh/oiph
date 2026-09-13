# DS.md — 数据结构说明

本文档描述 OIPH 当前的核心数据结构、状态模型与持久化格式。目录结构与配置文件格式见 FILES.md。

## 0. 序列化约定

| 场景 | 格式 | 位置 |
|---|---|---|
| 比赛/题目工程配置 | YAML | `<比赛目录>/config.yaml`、`<题目目录>/config.yaml` |
| 会话历史 | JSON | `<工程>/.oiph/sessions/<会话名>/main.json` 等 |
| agent 配置 | JSON | `~/.oiph/config/agents.json` |
| 费用预算 | JSON | `~/.oiph/config/limit.json` |
| UI 偏好 | JSON | `~/.oiph/config/ui.json` |

约定：

- 绝大多数字段带 `#[serde(default)]`，保证旧文件可读（向后兼容）。
- 运行时字段不序列化：`Problem.created_files`（`#[serde(skip)]`）、`Contest.loaded_problems`（`#[serde(skip)]`）。
- 枚举序列化风格按类型区分：`snake_case`（ProblemType / ProblemSource / ComponentStatus / SubtaskType）、`UPPERCASE`（Verdict）、内部标签（ComponentStatus 的 `state`）。

---

## 1. 核心领域模型（`src/model.rs`）

### 1.1 枚举

```rust
pub enum ProblemType {
    Traditional,              // 传统题
    Function,                 // 函数交互题（alias: interactive_lib，兼容旧名）
    InteractiveIO,            // IO 交互（serde rename: interactive_io，alias: interactive_i_o）
    AnswerOnly,               // 提交答案
}
impl ProblemType { pub fn label(&self) -> &'static str }   // 中文名

pub enum ProblemSource { Original, Moved, Adapted }        // 原创 / 搬运 / 改编

pub enum Verdict { Ac, Wa, Tle, Mle, Re, Partial }         // 序列化为 "AC"/"WA"/...
impl Verdict {
    pub fn as_str(&self) -> &'static str
    pub fn parse(s: &str) -> Option<Self>   // 宽松解析：大小写、英文全称、中文
}

pub enum SubtaskType { Sum, Min, Mul }                     // 子任务计分方式，默认 Sum
```

### 1.2 组件状态 `ComponentStatus`

组件状态是贯穿题目、比赛、工具执行进度的统一状态类型：

```rust
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ComponentStatus {
    NotStarted,                                              // { state: not_started }
    InProgress { progress: f32, message: String },           // { state: in_progress, progress: 0.3, message: "..." }
    Completed { timestamp: DateTime<Utc> },                  // { state: completed, timestamp: "..." }
    Failed { error: String },                                // { state: failed, error: "..." }
}

impl ComponentStatus {
    pub fn completed_now() -> Self
    pub fn in_progress(progress: f32, message: impl Into<String>) -> Self  // progress 自动 clamp 到 [0,1]
    pub fn failed(error: impl Into<String>) -> Self
    pub fn label(&self) -> String        // "未开始" / "进行中（30%）- ..." / "已完成 @ ..." / "失败：..."
    pub fn is_terminal_ok(&self) -> bool // 是否 Completed
    pub fn is_failed(&self) -> bool
}
```

### 1.3 状态聚合 `aggregate`

```rust
pub fn aggregate(statuses: Vec<ComponentStatus>) -> ComponentStatus
```

聚合优先级：**Failed > InProgress > Completed > NotStarted**。

- 空列表 → `NotStarted`。
- 有失败：合并错误信息（多个失败时 `N 个组件失败：a；b`）。
- 有进行中：进度取平均；消息合并去空。
- 全部 Completed：取最大时间戳。
- 否则（部分完成部分未开始）→ `NotStarted`。

### 1.4 `GetStatus` trait（重点）

```rust
pub trait GetStatus {
    fn get_status(&self) -> ComponentStatus;
}
```

实现一览：

| 实现类型 | 语义 |
|---|---|
| `ComponentStatus` | 返回自身克隆 |
| `Component` | 返回内部 `status` 字段 |
| `SolutionStatus` | 返回内部 `status` 字段 |
| `DataStatus` | 返回内部 `status` 字段 |
| `Problem` | `aggregate(self.component_statuses())` |
| `Contest` | 见下 |

`Problem::component_statuses()` 收集的组件（用于聚合题目整体状态）：

```
statement、std.status、sols[].status、data.status、
validator.status、checker.status、tutorial、interactive_lib.status（可选）
```

`Contest::get_status()`：

- `loaded_problems` 为空且 `problems` 也为空 → `NotStarted`；
- `problems` 非空但未加载 → `in_progress(0.0, "题目尚未加载")`（保守）；
- 否则 `aggregate(各题 get_status())`。

使用点（均为静态分发，无 `dyn GetStatus`）：

- `src/server.rs`：比赛 API 的 `status` 字段（`GetStatus::get_status(p).label()`）；
- `src/main.rs`：CLI REPL 状态展示；
- `src/project.rs`：`contest_status_text` / `problem_status_text` 的整体状态行。

### 1.5 组件结构

```rust
pub struct Component { pub status: ComponentStatus }

pub struct JudgingStatus {
    pub verdict: Verdict,
    pub score: Option<f64>,          // 缺省 AC / 100 分
}

pub struct SolutionStatus {
    pub name: String,
    pub file: Option<String>,        // 例如 "solutions/brute.cpp"
    pub expected: JudgingStatus,     // 期望评测结果（对拍用）
    pub status: ComponentStatus,
}

pub struct DataStatus { pub status: ComponentStatus }

pub struct DuplicateCheckResult {
    pub found: bool,
    pub matches: Vec<String>,
    pub checked_at: DateTime<Utc>,
    pub note: Option<String>,
}
```

### 1.6 题目工程

```rust
pub struct ProblemFiles {
    pub statement: String,        // 默认 "statement/zh_cn.md"
    pub down_dir: String,         // 默认 "statement/down"
    pub data_dir: String,         // 默认 "data"
    pub aux_dir: String,          // 默认 "auxiliary"
    pub solutions_dir: String,    // 默认 "solutions"
    pub std_file: Option<String>, // 默认 Some("solutions/std.cpp")
    pub tutorial: Option<String>, // 默认 None（约定路径 tutorial/zh_cn.md）
}

pub struct Problem {
    pub id: String,
    pub name: String,
    pub problem_type: ProblemType,
    pub source: ProblemSource,
    pub tags: Vec<String>,
    pub time_limit_ms: u64,          // 默认 1000
    pub memory_limit_mb: u64,        // 默认 512
    pub compile_flags: String,       // 默认 "-O2 -std=c++14"

    pub subtasks: Vec<Subtask>,                       // 测试点配置
    pub data_gen: BTreeMap<String, String>,           // case 名 -> generator 参数

    pub statement: ComponentStatus,
    pub std: SolutionStatus,                          // 默认 name=std, file=solutions/std.cpp
    pub sols: Vec<SolutionStatus>,                    // 其他做法（对拍/多做法预估）
    pub data: DataStatus,
    pub validator: Component,
    pub checker: Component,
    pub interactive_lib: Option<Component>,           // 函数交互题交互库
    pub tutorial: ComponentStatus,

    pub duplicate_check: Option<DuplicateCheckResult>,
    pub last_tested: Option<DateTime<Utc>>,
    pub files: ProblemFiles,

    #[serde(skip)]
    pub created_files: Vec<String>,   // 运行时：新建题目时创建的骨架文件，不持久化
}

pub struct Subtask {
    pub score: f64,
    pub stype: SubtaskType,       // sum/min/mul
    pub cases: Vec<String>,       // 测试点名（对应 data/<name>.in 或 data_gen 的 key）
    pub pretest: bool,
    pub sample: bool,
    pub depend: Vec<u32>,         // 依赖的 subtask 序号（1-based）
}
```

### 1.7 比赛

```rust
pub struct ContestConfig {
    pub start_time: Option<DateTime<Utc>>,
    pub duration_min: Option<u64>,
    pub notes: Option<String>,
}

pub struct Contest {
    pub id: String,
    pub name: String,
    pub problems: Vec<String>,        // 持久化：题目目录名列表
    pub config: ContestConfig,
    pub created_at: DateTime<Utc>,

    #[serde(skip)]
    pub loaded_problems: Vec<Problem>, // 运行时：由 project::load_contest 填充
}
```

---

## 2. LLM 通信（`src/client.rs`）

```rust
pub struct Message {
    pub role: String,                                   // system / user / assistant / tool / compaction
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    #[serde(rename = "reasoning_content", skip_serializing)]
    pub reasoning: Option<String>,                      // 只读，不回传给模型
}

pub struct ToolCall { pub id: String, #[serde(rename="type")] pub kind: String, pub function: FunctionCall }
pub struct FunctionCall { pub name: String, pub arguments: String }   // arguments 为 JSON 字符串

pub struct Tool { #[serde(rename="type")] pub kind: String, pub function: FunctionDef }
pub struct FunctionDef { pub name: String, pub description: String, pub parameters: Value }

pub struct ChatUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cache_hit_tokens: Option<u64>,
    pub cache_miss_tokens: Option<u64>,
}

pub struct ChatResult {
    pub message: Message,
    pub usage: Option<ChatUsage>,
    pub interrupted: bool,
}
```

Token 估算：`estimate_message_tokens(&Message) -> f64`（CJK 约 0.6 token/字，其他约 0.25 token/字符 + 每条固定开销）。

---

## 3. 会话持久化（`src/session.rs`）

```rust
pub struct Session {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<Message>,
    pub children: Vec<ChildRef>,     // 子 agent 会话引用
    pub usage: TokenUsage,           // 该会话累计用量
}

pub struct SubSession { pub agent: String, pub messages: Vec<Message> }

pub struct ChildRef { pub filename: String, pub agent: String, pub summary: String }

pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cache_hit_tokens: Option<u64>,
    pub cache_miss_tokens: Option<u64>,
}

pub struct SessionMeta { pub name: String, pub updated_at: DateTime<Utc>, pub messages: usize, pub current: bool }
```

磁盘布局（工程目录 `.oiph/sessions/`）：

```
.oiph/sessions/
├── current                        # 当前会话名（纯文本）
└── <会话名>/
    ├── main.json                  # 主 Session
    ├── sub-1.json, sub-2.json ... # 子 SubSession（按产生顺序编号）
    └── pending_user.json          # 已收到但尚未并入历史的用户消息（崩溃恢复用）
```

---

## 4. 配置与计价

### 4.1 agent 配置（`src/config.rs`，`~/.oiph/config/agents.json`）

```rust
pub struct AgentConfig {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,                    // 必需（缺失时调用报错提示配置）
    pub prompt: Option<String>,                   // 提示词文件路径；compactor 可省略
    pub reasoning: Option<bool>,                  // 思考模式开关
    pub price: Option<PriceConfig>,               // 固定单价（优先）
    #[serde(rename = "price-policy")]
    pub price_policy: Option<String>,             // 目前仅 "auto"
    pub max_context: Option<u64>,                 // 超限先压缩
}
pub type AgentsConfig = HashMap<String, AgentConfig>;   // supervisor/statement/solution/auxiliary/searching/compactor

pub struct AgentSettings {                        // 运行时解析结果
    pub model: Option<String>,
    pub reasoning: Option<bool>,
    pub max_context: u64,
    pub pricing: Pricing,
}
```

解析回退链：自身显式值 → 环境变量 `OPENAI_BASE_URL` / `OPENAI_API_KEY` / `OPENAI_MODEL`。

### 4.2 提示词（`src/prompts.rs`）

```rust
pub struct AgentPrompts {
    pub supervisor: String,
    pub statement: String,
    pub solution: String,
    pub auxiliary: String,
    pub searching: String,
}
```

### 4.3 计价（`src/pricing.rs`）

```rust
pub struct PriceConfig {
    pub input: f64,     // 输入（缓存未命中）价格 / M token
    pub hit: f64,       // 输入（缓存命中）价格 / M token
    pub output: f64,    // 输出价格 / M token
    pub currency: String,   // ISO 代码，如 CNY/USD
}

pub enum Pricing {
    None,                    // 不估算
    Fixed(PriceConfig),      // 固定单价
    DeepSeek,                // DeepSeek 峰谷计价（北京时间周一至周五 9-12/14-18 为高峰）
}

pub struct Cost { pub currency: String, pub amount: f64 }
```

### 4.4 费用预算（`src/budget.rs`，`~/.oiph/config/limit.json`）

```rust
pub struct BudgetFee {
    pub limit: f64,       // 预算上限
    pub used: f64,        // 已用（预算货币）
    pub warn: f64,        // 剩余低于该值告警
    pub currency: String, // ISO 货币代码
}
// 文件不存在 => None，预算功能未启用
```

---

## 5. 工作区快照（`src/snapshot.rs`）

```rust
pub struct SnapshotPoint {
    pub hash: String,     // 快照的 git tree hash
    pub msg_len: usize,   // 对应时刻的对话消息数（undo/redo 时同步回退对话）
}

pub struct SnapshotStore {
    git_dir: PathBuf,     // <会话目录>/snapshot/.git（独立 git 仓库）
    work_tree: PathBuf,   // 被跟踪的工作区（比赛工程目录）
}
```

快照机制：`git add -A` + `git write-tree` 记录 tree hash（不产生 commit）；恢复用 `read-tree` + `checkout-index` 并删除未被跟踪的新文件。通过 `--git-dir`/`--work-tree` 与工程自身的 git 仓库隔离，index 使用独立的 `index.tmp`。

undo/redo 栈存于 `App`（`undo_stack` / `redo_stack: Mutex<Vec<SnapshotPoint>>`）。

---

## 6. 运行时状态

### 6.1 `App`（`src/state.rs`）

| 字段 | 类型 | 说明 |
|---|---|---|
| `root` | `PathBuf` | 启动工作目录 |
| `current` | `Mutex<Option<PathBuf>>` | 当前比赛目录（无比赛为 None） |
| `max_steps` | `usize` | 每回合最大工具轮数 |
| `dup_backend` | `Backend` | 查重后端（cpret / yuantiji） |
| `prompts` | `Mutex<AgentPrompts>` | 各 agent 系统提示词 |
| `agent_clients` | `Mutex<HashMap<String, Client>>` | per-agent HTTP 客户端 |
| `agent_settings` | `Mutex<HashMap<String, AgentSettings>>` | per-agent 模型/计价设置 |
| `compactor_prompt` | `Mutex<String>` | 压缩提示词（未配置用内置默认） |
| `undo_stack` / `redo_stack` | `Mutex<Vec<SnapshotPoint>>` | 快照栈 |
| `ask_answer` | `Mutex<Option<UnboundedSender<Value>>>` | ask_user 问卷回传通道 |
| `budget` | `Mutex<Option<BudgetFee>>` | 费用预算 |

### 6.2 `ServerState`（`src/server.rs`）

| 字段 | 类型 | 说明 |
|---|---|---|
| `app` | `Arc<App>` | 应用状态 |
| `messages` | `Arc<Mutex<Vec<ChatMessage>>>` | 当前会话消息（回合期间被 run_turn 持有） |
| `current_session` | `Arc<Mutex<Option<String>>>` | 当前会话名 |
| `cancel` | `CancelFlag` | 中止标志（AtomicBool + Notify） |
| `pending_usage` | `Mutex<TokenUsage>` | 本回合未落盘用量 |
| `saved_usage` | `Mutex<TokenUsage>` | 已持久化用量基线（状态栏） |
| `pending_user` | `Mutex<Vec<ChatMessage>>` | 已收到但尚未并入历史的用户消息 |
| `persist_lock` | `Mutex<()>` | 串行化"收到即落盘"与 session 切换/新建 |

### 6.3 其他运行时类型

```rust
// src/agent.rs
pub enum Role { Supervisor, Searching, Statement, Solution, Auxiliary }
pub struct AgentDeps { pub max_steps: usize }
pub struct TurnResult { pub text: String, pub interrupted: bool, pub usage: Option<ChatUsage> }
pub type UsageSink = Arc<Mutex<Option<ChatUsage>>>;   // 回合共享用量累积器

// src/term.rs
pub struct CancelFlag { /* AtomicBool + Notify */ }

// src/dupcheck.rs
pub enum Backend { Cpret, Yuantiji }
```

---

## 7. 状态语义速查

| 状态 | 含义 | 典型来源 |
|---|---|---|
| `NotStarted` | 未开始 | 新建题目/组件 |
| `InProgress{progress,message}` | 进行中（可带进度与说明） | 子 agent 执行中、集成测试运行中 |
| `Completed{timestamp}` | 已完成（成功） | 题面/std/数据/校验器完成、测试通过 |
| `Failed{error}` | 失败（带错误信息） | 编译失败、测试错误、导出失败 |

- 题目整体状态 = 各组件状态聚合（`Problem::get_status()`）。
- 比赛整体状态 = 各题目状态聚合（`Contest::get_status()`）。
- `GetStatus` 是"任何带状态的实体 → ComponentStatus"的统一抽象，展示层只需调用 `.label()`。
