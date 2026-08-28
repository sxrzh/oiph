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
│  - 解析用户意图（自然语言命令）                               │
│  - 任务队列管理                                            │
│  - 子 Agent 调度与结果汇总                                 │
└──────────┬──────────┬──────────┬──────────┬───────────────┘
           │          │          │          │
    ┌──────▼─────┐┌───▼─────┐┌───▼─────┐┌───▼─────┐
    │statement   ││solutions││auxiliary││searcher │ ... 子 Agent
    │-agent      ││-agent   ││-agent   ││-agent   │
    └────────────┘└─────────┘└─────────┘└─────────┘
           │          │          │          │
    ┌──────▼──────────▼──────────▼──────────▼───────┐
    │              外部服务 / 工具                   │
    │  LLM API | Polygon API | 搜索引擎 | 文件系统    │
    └────────────────────────────────────────────────┘
```
子 agent 包括 searching-agent，statement-agent，solutions-agent，auxiliary-agent 等

- supervisor-agent 负责根据用户命令规划任务、检查任务完成质量
- searching-agent 负责在网上搜索冷门题目及相关资料（测试数据、辅助程序等）
- statement-agent 负责写题面或改编题面
- solution-agent 负责写 std 及暴力等其他 solutions
- auxiliary-agent 负责写 generator、checker、validator、interactive_lib 等辅助程序，并造数据

其中 supervisor 和每一个子 agent 都要支持工具调用（联网搜索（调用 bing），读/写文件，bash）、RAG 知识库、Skills
支持用户通过命令行自行添加知识库文档（纯文本）

在查找原题时调用工具，这个工具之后实现，现在统一返回没有原题

检查数据、std 正确性以后可能会直接通过工具调用 tuack-ng 工具（注意不是 tuack），可以先统一返回 true

agent 的设计可以参考 ref/simple-agent

supervisor 在接到用户要求后，首先确定每道题目是原创 idea 还是搬运
- 如果是原创 idea：
    1. 如果 idea 是某些算法/数据结构/技巧，则由 statement-agent 根据用户指定的难度，可以结合 OI 中其他算法/数据结构/技巧，出一道题，写一个形式化题面；
    2. supervisor 先调用工具查找原题，报告用户查找情况并询问是否继续，如果不继续则返回 1 重写，否则继续；
    3. 由 statement-agent 根据 idea 和用户要求（例如：简洁/形式化/以某个东西为背景）写完整的题面；
    4. 之后由 auxiliary-agent 写 generator、checker、validator、interactiv_lib（如果需要），并造数据；
    5. 之后 supervisor 调用工具检查数据正确性，如果不正确则返回 4，正确则继续；
    6. 之后由 solution-agent 阅读题面、设计算法并写 std；
    7. solution-agent 写完 std 之后 supervisor 调用工具检查 std 正确性，如果不正确则返回 6，正确则继续；
    8. 之后由 solution-agent 写可能的其他正解、错误解法和暴力解法、部分分解法，并预估它们的评测结果和得分；
    9. supervisor 调用工具检查这些其他解法的评测结果和得分是否符合 solution-agent 估计，不符合则返回 8，符合则继续；
    10. 将 solution-agent 的对话历史传给 statement-agent 写题解；
- 如果是搬运/改编题目：
    1. 如果用户没有指定搬运来源，调用 searching-agent 从冷门来源中随机查找题目，其中需要根据网站标签（如果有）、阅读已通过的代码（如果容易找到）、自己阅读题面等方式估计题目难度和所需知识点（算法、数据结构、技巧），如果不符合用户要求则重新查找；
    2. 确定符合用户要求后，searching-agent 全网搜索这道题的 std、测试数据、辅助程序等；
    3. 如果用户需要改编，由 statement-agent 改编题面，如果改变了做法，则按照原创 idea 的 4 之后的流程继续；否则：
    4. supervisor 依次检查辅助程序、测试数据、std 是否存在且完整，如果遇到问题则调用对应的 agent 编写相关内容；
    5. 由 solution-agent 写可能的其他正解、错误解法和暴力解法、部分分解法，并预估它们的评测结果和得分；
    6. supervisor 调用工具检查这些其他解法的评测结果和得分是否符合 solution-agent 估计，不符合则返回 8，符合则继续；
    7. 如果 std 是搜索到的，由 solution-agent 阅读 std 代码写题解；
       如果 std 是 solution-agent 写的，把 solution-agent 的对话历史传给 statement-agent 写题解；

statement-agent 书写题面需要满足以下要求：
- 题面用 Markdown 书写，符合以下规范：
    - 对于中文题面，全文使用全角中文标点符号，句号“。”不能省略
    - 数学公式、运算符、变量、常数等须使用 LaTeX 格式；而普通英文单词、题目或算法名称、人名等则不应使用 LaTeX。
    - 中文与英文、数字或 LaTeX 公式之间需加半角空格，但中文标点符号与其前后相邻的英文、数字或公式之间不加空格。
- 题面分为题目背景（与题目数学本质完全无关的内容）、题目描述（必要时可以增加“形式化题意”）、输入格式、输出格式、样例、数据范围与提示
    - 应当保证即使不看题目背景也完全不影响做题
    - 对于函数交互，交互格式写在题目描述里，交互题的输入、输出格式是交互库的输入、输出格式
    - 对于 IO 交互，交互格式写在输入格式、输出格式里
    - 数据范围应当在“数据范围与提示”部分，而不是题目描述部分
- 知识库中 statement_req.md 有更详细的要求

searching-agent 暂无额外要求。

auxiliary-agent 写程序满足以下要求：
- 所有辅助程序基于 testlib.h，符合 C++14 标准编写

solution-agent 写程序满足以下要求：
- 所有的 solution 应该是单个文件，符合 C++14 标准
- 只能使用 C++ 标准库和 `pbds` 库、`bits/extc++` 库等 GNU 扩展
- 不要在程序中创建进程、线程，不要使用 `system` 函数
- 变量名不要太长，但是不要大量使用无意义的变量名

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

agent 的架构可以参考 simple-agent 目录