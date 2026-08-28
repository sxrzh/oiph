我要用 Rust 写一个信息学竞赛模拟赛组题助手 agent，初步描述如下见 AGENTS.md

## 架构示意
```text
┌─────────────────────────────────────────────────────────────┐
│                     Web UI (前端)                           │
│  标签页 | 按钮 | 命令输入 | 状态面板 | 日志               │
└─────────────────────┬───────────────────────────────────────┘
                      │ IPC (Tauri) / HTTP (Axum)
┌─────────────────────▼───────────────────────────────────────┐
│                    Supervisor Agent (Rust)                  │
│  - 解析用户意图（按钮点击 / 自然语言命令）                 │
│  - 任务队列管理                                            │
│  - 子 Agent 调度与结果汇总                                 │
└──────────┬──────────┬──────────┬──────────┬───────────────┘
           │          │          │          │
    ┌──────▼─────┐┌───▼────┐┌───▼────┐┌───▼────┐
    │statement   ││solutions││generator││checker │ ... 子 Agent
    │-agent      ││-agent   ││-agent   ││-agent  │
    └────────────┘└─────────┘└─────────┘└────────┘
           │          │          │          │
    ┌──────▼──────────▼──────────▼──────────▼───────┐
    │              外部服务 / 工具                   │
    │  LLM API | Polygon API | 搜索引擎 | 文件系统  │
    └────────────────────────────────────────────────┘
```
其中 supervisor 和每一个子 agent 都要支持工具调用、RAG 知识库、Skills

在查找原题时调用工具，这个工具之后实现，现在统一返回没有原题

检查数据、std 正确性以后可能会直接通过工具调用 tuack-ng 工具（注意不是 tuack），可以先统一返回 true

## 通信协议
前端 → Supervisor：通过 Tauri IPC 或 HTTP 发送任务请求（含题目 ID、任务类型、参数）

Supervisor → 前端：通过 WebSocket 或事件回调推送状态更新

Supervisor → 子 Agent：通过 Rust 内部函数调用或消息通道（如 tokio::sync::mpsc）

子 Agent → 外部：调用 LLM API、Polygon API、搜索引擎等

## 数据模型建议

在本系统中“比赛”是工程最高层的对象，数据结构上不区分比赛日，比赛下有多个题目

### 比赛（Contest）
```rust
struct Contest {
    id: String,
    name: String,
    problems: Vec<Problem>,
    config: ContestConfig, // 时间、评测参数等
    created_at: DateTime,
}
```
### 题目（Problem）
```rust
struct Problem {
    id: String,
    name: String,
    prob_typ: ProblemType,      // Traditional | InteractiveLib | InteractiveIO | AnswerOnly | Function
    
    source: ProblemSource, // Original |搬运 | Adapted
    difficulty: Difficulty,     // 包括 入门，普及-，普及，提高-，提高，提高+/省选-，省选/NOI-，NOI，NOI+/CTSC 几档
    tags: Vec<String>,          // 题目知识点标签
    
    // 各组件状态
    statement: ComponentStatus,       // 题面写作
    std: SolutionStatus,              // 标准答案 std 编写
    sols: Vec<SolutionStatus>,        // 除 std 外的其他答案程序，包括可能的正确答案（AC）、输出错误（WA）、运行错误（RE）
    data: DataStatus,                 // 造数据（generator 或具体数据）
    validator: Component,       // validator，一个程序用来检验造出的数据是否符合输入格式
    checker: Component,         // checker，一个程序用来检验被测试程序的输出是否符合要求
    interactive_lib: Option<ComponentStatus>,  // 交互库（如果需要）
    tutorial: ComponentStatus,        // 题解
    
    duplicate_check: Option<DuplicateCheckResult>,      // 查重
    
    // 文件路径
    files: ProblemFiles,
}
```

### 组件状态

```rust
struct SolutionStatus {
    expected: JudgingStatus,
}
```

```rust
struct DataStatus {
    data_type: DataType,    // Blob | Generated
}
```

需要实现一个 `GetStatus` trait，以上这些组件类型（包括 `Problem` 和 `Contest`）都实现这个 trait，调用一个 `get_status` 函数会返回以下类型：

```rust
enum ComponentStatus {
    NotStarted,
    InProgress { progress: f32, message: String },
    Completed { timestamp: DateTime },
    Failed { error: String },
}
```

## 目录结构  

contest_name
    - problem_name_a
        - statement
            - zh_cn.md
            - down
                - 下发文件
        - data
            - config.yaml
            - 1.in
            - 1.ans
            - ... 
            - (或：config.yaml 和 generator.cpp)
    - problem_name_b
        - ...

WebUI 界面、Polygon API、各种兼容导出 先不写，先写一个 CLI 实现以上功能，
用户可以和 supervisor agent 对话，supervisor 以工具调用形式调用其他 Agent

用户可以通过 `/` 开头的指令查看当前工程状态（比赛、每个题目每个部件的状态）

std、sols 只需要考虑 C++ 程序，generator、validator、checker、interactive_lib 统一用 C++ 基于 testlib 编写。
比赛和题目的数据存储在本地文件，配置存储在（比赛/题目）对应目录下的 config.yaml