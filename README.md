# OI-contest-preparer

An agent to help with OI contest problem preparing.

As a part-time or professional OI trainer, you may be frustrated preparing a training contest, which contains several repeated and time-consuming works:
- searching remote contests rarely known by your contestants and do some modification
- searching for official testcases or making it by your own
- searching for std solution or solving it yourself
- packing into packages in formats like Polygon, lemon, ...

This agent is here to help you out! Based on high problem-solving ability of LLMs and automated pipelines, the workflow above will be done fast like a lightning.

## 架构

多 Agent 系统，与用户交互的是 supervisor，它以工具调用的形式调度子 Agent：

- **supervisor**：根据用户命令规划任务（原创 / 搬运流程）、调度子 Agent、检查质量、汇报
- **searching-agent**：搜索冷门题目与资料（std、测试数据、辅助程序），估计难度与知识点
- **statement-agent**：出题 / 写题面 / 改编题面 / 写题解
- **solution-agent**：设计算法写 std 及其他解法，预估评测结果
- **auxiliary-agent**：写 generator / checker / validator / interactive_lib 并造数据

所有 Agent 支持工具调用（bash、读写文件、Bing 搜索、fetch_url、RAG 知识库、Skills）。

## 构建与运行

```sh
cargo build

# GUI 模式（默认，无子命令时启动）
./target/debug/preparer
# 浏览器访问 http://localhost:17217

# CLI 模式
./target/debug/preparer cli           # 进入交互式 REPL
./target/debug/preparer cli "出题"    # 单次任务

# 指定 GUI 端口
./target/debug/preparer --port 8080

# 其他 CLI 子命令
./target/debug/preparer status        # 查看工程状态
./target/debug/preparer test [题目id] # 集成测试
./target/debug/preparer export lemon  # 导出 LemonLime
./target/debug/preparer kb add <文件> # 知识库管理
./target/debug/preparer session list # 会话管理

# 知识库管理（两级）
- 全局知识库：`~/.oiph/kb/`；工程知识库：`<比赛工程>/.oiph/kb/`（检索时两者合并）
- 仓库内 `assets/kb/` 的文档（题面规范、题目来源、NOI 大纲）在启动时自动种子到全局知识库
- `kb add` 默认加到工程知识库（无比赛工程则全局），`-g/--global` 强制全局

./target/debug/preparer kb add assets/kb/statement_req.md
./target/debug/preparer kb add some_doc.txt -g
./target/debug/preparer kb list
./target/debug/preparer kb clear        # 默认清工程，不存在则清全局
./target/debug/preparer kb clear -g
./target/debug/preparer kb search "题面规范 全角标点"

# skills 管理（两级：全局 ~/.oiph/skills，工程 <比赛工程>/.oiph/skills）
./target/debug/preparer skill list
./target/debug/preparer skill show duipai
./target/debug/preparer skill add my_skill.md mine      # 安装到工程（无比赛则全局）
./target/debug/preparer skill add my_skill.md mine -g   # 安装到全局
./target/debug/preparer skill delete mine

# 会话管理（存于 <比赛工程>/.oiph/sessions/）
./target/debug/preparer session list
./target/debug/preparer session new          # 自动命名
./target/debug/preparer session new myname
./target/debug/preparer session use myname
./target/debug/preparer session delete old
./target/debug/preparer session export        # 导出当前会话为 markdown
./target/debug/preparer session show myname   # 打印会话内容
```

常用参数：`-m 模型名`、`--max-steps 40`、`-c 比赛目录`、`--dup-backend cpret|yuantiji`、`--port 17217`（GUI 端口）、`--embedding-model`（省略则用内置离线哈希 embedding）。

## GUI 模式

无子命令启动时默认进入 GUI 模式，浏览器访问 `http://localhost:17217`：

- **左侧题目区**：题目选项卡 + 基本信息/题面/题解/数据/辅助程序/解法标签页
- **右侧对话区**：与 supervisor agent 实时流式对话（含思维链）、session 切换、中止按钮
- **顶部菜单**：导出 LemonLime、集成测试
- **底部状态栏**：工程路径、Token 用量

## CLI 模式

`preparer cli` 进入交互式 REPL，支持 `/` 开头的本地指令：

| 指令 | 作用 |
| --- | --- |
| `/help` | 帮助 |
| `/status [题目id]` | 查看比赛整体 / 单个题目各组件状态 |
| `/contest list` `/contest new <名>` `/contest use <目录>` | 比赛管理 |
| `/problem list` `/problem add <id> [类型]` `/problem show <id>` | 题目管理 |
| `/kb add <文件> [global]` `/kb list` `/kb clear [global]` | 知识库 |
| `/skill list` `/skill show <名>` | skills |
| `/session list` `/session new [名]` `/session use <名>` `/session delete <名>` `/session export [名] [路径]` | 会话 |
| `/exit` | 退出 |

## Skills

Skill 是一个目录，内含 `SKILL.md`（YAML frontmatter：`name`、`description`，后接指令正文）：

```yaml
---
name: duipai
description: OI 题目对拍：用暴力解与被测程序对比找反例，限制组数与运行时间。
---
# 正文指令
```

- 全局 skills：`~/.oiph/skills/`；工程 skills：`<比赛工程>/.oiph/skills/`（同名时工程覆盖全局）
- 各 Agent 的系统提示词中会嵌入每个 skill 的 `name: description`；需要执行时用 `load_skill` 工具读取全文
- 仓库内置 `assets/skills/duipai/`（对拍 skill）在启动时自动种子到全局 skills

## 会话（Session）

- supervisor 对话自动保存为 JSON，存于 `<比赛工程>/.oiph/sessions/<名>.json`
- 在比赛目录下启动 `preparer` 时自动加载上一次的会话（按 `current` 指针，缺失则取最近修改的）
- 每轮对话后自动保存；`/session new|use` 切换前也会保存当前会话
- `preparer session export` 或 `/session export` 可导出为 markdown

## 导出

```sh
# 导出为 LemonLime 格式
./target/debug/preparer export lemon [输出目录]

# 默认输出到 <比赛目录>/<比赛名>_lemon/
```

LemonLime 导出内容包括：
- `<比赛名>.cdf`：比赛配置 JSON（含 subtasks、依赖、SPJ 配置等）
- `data/<题目id>/`：测试数据文件（.in/.ans）
- `data/<题目id>/spj.cpp` + `testlib.h`：SPJ（checker）使用 LemonLime 兼容的 testlib.h
- `data/<题目id>/grader.cpp` + `inter.h`：交互题 grader 与交互库头文件
- `compile_spj.bat`：编译所有 SPJ 的批处理脚本（Windows g++）

## 工程目录与配置

见 FILES.md。比赛与题目的数据存本地文件，配置存对应目录的 `config.yaml`；组件状态实现 `GetStatus` trait（见 src/model.rs）。

## 未实现（桩）

- 数据 / std / 解法检查 `check_data` / `check_std` / `check_solutions`：统一返回通过（后续接入 tuack-ng）
- Web UI、Polygon API、其他 OJ 格式导出（SYZOJ/HydroOJ 等）

## 查重（原题查找）

- `duplicate_check` 工具支持两个后端：**cpret.online**（默认）与 yuantiji.ac
- CLI `--dup-backend cpret|yuantiji` 设置默认后端；工具的 `backend` 参数可逐次覆盖
- 内置频率控制（≥3s 间隔），两个后端共享
