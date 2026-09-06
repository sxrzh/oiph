# OIPH 设计文档 · 摘要

> 配套文档：`docs/reports/DESIGN.md`（全文）；架构图见 `architecture.png`。

## 项目定位

OIPH 是面向 **OI 模拟赛组题**场景的 Agent 系统：用户用自然语言指挥 **supervisor** Agent，由其调度 4 个专用子 Agent（searching / statement / solution / auxiliary）与 20+ 领域工具，覆盖"原创/搬运/改编题目 → 查重 → 题面 / std / 题解 → 造数据 → 集成测试 → 导出 OJ 包"的完整流水线。交付物是可直接评测的比赛产物，而非代码片段。

## 痛点与现状

- 组一场比赛是以天计的重复苦役：搜冷门题并评估"学员见过率"、补官方数据与 std、写 generator/validator/checker/交互库、适配四类题型的评测差异、按 LemonLime/SYZOJ/HydroOJ 等格式导出、控制 LLM 费用。
- 通用编程 Agent 缺 OI 领域知识；孤立 AI 小工具无端到端工作流且不互通；Polygon 等平台流程繁琐、对国内学校教练不友好。

## 核心定制方案

1. **多角色 Agent 编排**：supervisor 规划/派活/验收，子 Agent 各司其职（搜题/写题面/写辅助程序/写解法）；每角色独立提示词与模型配置，便于将不同难度的任务交给不同 LLM；工具按领域定制，包括比赛工程管理、集成测试、bash 超时（防止 agent 测试死循环程序阻塞工作流）。
2. **可评测的题目工程 + 集成测试器**：`Problem` 领域模型（题型/来源/subtasks/组件状态机）持久于 config.yaml；测试器真实复现评测——按题型分派管线：传统题 io 重定向、函数交互题联合编译 `xxx.cpp interactive_lib.cpp`、IO 交互题用双向管道、提交答案题目录输出直接 checker 比对；sol 结果与预期 verdict 比对；全部子进程超时保护、checker 缺失即报错。
3. **会话层**：WS 实时流式（正文/思维链/工具调用）；bare-git 快照实现 undo/redo（同步回退工作区与对话）；
4. **计费与预算**：按 agent 手动单价或 base_url 自动识别（例如 DeepSeek 峰谷计价）；状态栏三级用量模型（基线+回合+流式估算）；limit.json 预算 + warn 告警（菜单栏持久警示 + sweetalert）+ frankfurter 实时汇率换算 + `fee reset`。
5. **领域知识**：内置 OI 相关 RAG 知识库（题面规范、冷门题目来源、NOI 大纲等）与可发现/按需加载的 Skills（OJ API、OI 技巧等），agent 检索即所得。

## 架构与数据流

- 分层：Web GUI（React，页面外置于 `~/.oiph/frontend/dist`）→ axum REST+WS 服务端（共享消息/会话/取消标志/用量）→ supervisor 循环与子 Agent → 工具层 → 领域层（工程模型、集成测试、导出、会话快照、知识库）→ 外部（LLM API、Bing、查重站、汇率站）。
- 一轮对话主链：chat → 建会话 → 上下文压缩检查 → SSE 模型调用（实时回推）→ 工具循环（先快照、bash 计时）→ 子 Agent 内嵌回合 → 最终回答 → 用量/费用入账 → 增量落盘 + 前端刷新。
- 关键结构：`Message`（含只进不出的 reasoning）、`Problem`/`ComponentStatus`、`Session`/`TokenUsage`、`SnapshotPoint{hash,msg_len}`、`AgentConfig`（自身→环境变量回退）、`Pricing` 枚举、`BudgetFee{limit,used,warn,currency}`。

## 技术选型要点

- **Rust + tokio**：流式/并发/子进程控制精确，单二进制分发；`axum`(ws) + `tower-http ServeDir` 提供 REST/WS/静态页；`reqwest`(rustls) SSE 流式；手写 SSE 解析以兼容 reasoning_content/tool_calls/usage；CLI 用 clap + rustyline + crossterm（双 Esc 打断）；serde/serde_yaml 承载双形态配置（alias 兼容旧字段）；chrono 实现峰谷时段判定；scraper 轻量抓取。
- **前端**：React 19 + Vite + TS（WS 协议联合类型）；react-markdown + KaTeX 渲染 OI 题面；CodeMirror 6 编辑/只读查看；sweetalert2 交互。
- **取舍**：不用 LLM SDK（换取打断/用量/计价完全控制）；知识库用本地哈希 embedding（离线零成本）；testlib 等第三方文件外置 vendor 不内嵌（可免重编译升级）。
