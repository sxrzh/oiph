# OIPH - 信息学竞赛组题助手 Agent

OIPH（OI Preparer Helper）是一个专为辅助信息学竞赛模拟赛组题工作全流程而设计的 Agent 工具，工作流涵盖找题/出题、造数据、写题解、验题、集成测试、打包等。

## 前言

每一场被选手记住的 OI 模拟赛，背后都站着一个在深夜里反复打磨的组题人。

他要在浩如烟海的题库里翻出几道"冷门得恰到好处"的题——既不曾被选手撞见，又恰好配得上他们的野心；要为每道题打磨题面、推敲数据、写出一份经得起质疑的 std；要和 generator、checker、validator、交互库们逐一搏斗，直到它们学会在评测机上沉默地各司其职；再补上题解、打包、导出……如此往复。这些劳动并非没有意义，只是它们本该让位于一件更重要的事：把一场比赛真正打磨成一次思维的冒险。

OI 组题，不该是一场让人望而生畏的苦役。

这就是 OIPH 的由来：把"组一场高质量模拟赛"从一项持续数周的事务性工程，压缩成一段与助手的对话。它未必比经验老到的出题人更懂出题——但它可以不知疲倦地翻遍冷门角落，可以把你灵感忽至的 idea 迅速补全成一道完善的题目，可以替你试过几百种数据的边界，一丝不苟地补齐每一份代码与配置，并在几分钟内完成你过去要耗尽整个下午的重复劳动。它是一支召之即来的团队：有人去搜题，有人写题面，有人写 std，有人造数据、写校验器和评测器——而那位真正的组题人，依然稳稳坐在驾驶席上：方向由你定，质量由你把关，每一道关键抉择都留给你。

它站在训练者这一边，让"好训练"不再稀缺。

## 从源码构建

```sh
cargo build

# GUI 模式（默认，无子命令时启动）
./target/debug/oiph
# 浏览器访问 http://localhost:17217

# CLI 模式
./target/debug/oiph cli           # 进入交互式 REPL
./target/debug/oiph cli "出题"    # 单次任务

# 指定 GUI 端口
./target/debug/oiph --port 8080

# 其他 CLI 子命令
./target/debug/oiph status        # 查看工程状态
./target/debug/oiph test [题目id] # 集成测试
./target/debug/oiph export lemon  # 导出 LemonLime
./target/debug/oiph kb add <文件> # 知识库管理
./target/debug/oiph session list # 会话管理

# 首次使用：初始化 ~/.oiph（安装内置 skills/kb/prompts + vendor，生成 limit.json 与 agents.json）
./target/debug/oiph init [--force] [--assets <dir>]  # 或继续用 ./init.sh（兼容入口）

# vendor（~/.oiph/vendor/：testlib.h、testlib_lemon.h，init.sh 安装）
# 运行时优先使用 vendor 中的版本（可自行升级），缺省回退内置版本

# agent 配置（~/.oiph/config/agents.json：每个 agent 的 base_url / api_key / prompt，
# 以及可选的 reasoning（思考模式）、price（固定单价 {input,hit,output,currency}，货币/M token）、
# price-policy: "auto"（按 base_url 自动识别供应商计价，支持 DeepSeek 峰谷计价，
# GLM 不估算费用；price 与 price-policy 都缺省时即 auto）、
# max_context（最长上下文 token 估算，超过则自动调用 compactor 压缩，默认 1048576）。
# 另有可选的 "compactor" 项指定上下文压缩模型与提示词，缺省回退 supervisor）
# 提示词编辑（存于 ~/.oiph/config/prompts/<agent>.md）
./target/debug/oiph prompt update statement new_prompt.md  # 从文件替换
./target/debug/oiph prompt edit statement                  # vim 编辑（git commit 风格）
./target/debug/oiph prompt edit solution code              # 指定编辑器

# 知识库管理（两级）
- 全局知识库：`~/.oiph/kb/`；工程知识库：`<比赛工程>/.oiph/kb/`（检索时两者合并）
- `init.sh` 把 `assets/kb/` 构建到全局知识库（来源标签为 `<builtin>/...`）
- `kb add` 默认加到工程知识库（无比赛工程则全局），`-g/--global` 强制全局

./target/debug/oiph kb add assets/kb/statement_req.md
./target/debug/oiph kb add some_doc.txt -g
./target/debug/oiph kb list
./target/debug/oiph kb clear        # 默认清工程，不存在则清全局
./target/debug/oiph kb clear -g
./target/debug/oiph kb search "题面规范 全角标点"

# skills 管理（两级：全局 ~/.oiph/skills，工程 <比赛工程>/.oiph/skills）
./target/debug/oiph skill list
./target/debug/oiph skill show duipai
./target/debug/oiph skill add my_skill.md mine      # 安装到工程（无比赛则全局）
./target/debug/oiph skill add my_skill.md mine -g   # 安装到全局
./target/debug/oiph skill delete mine

# 会话管理（存于 <比赛工程>/.oiph/sessions/）
./target/debug/oiph session list
./target/debug/oiph session new          # 自动命名
./target/debug/oiph session new myname
./target/debug/oiph session use myname
./target/debug/oiph session delete old
./target/debug/oiph session export        # 导出当前会话为 markdown
./target/debug/oiph session show myname   # 打印会话内容
```

常用参数：`-m 模型名`、`--max-steps 40`、`-c 比赛目录`、`--dup-backend cpret|yuantiji`、`--port 17217`（GUI 端口）、`--embedding-model`（省略则用内置离线哈希 embedding）。

## GUI 模式

无子命令启动时默认进入 GUI 模式，浏览器访问 `http://localhost:17217`：

- **左侧题目区**：题目选项卡 + 基本信息/题面/题解/数据/辅助程序/解法标签页
- **右侧对话区**：与 supervisor agent 实时流式对话（含思维链）、session 切换、中止按钮
- **顶部菜单**：导出 LemonLime、集成测试
- **底部状态栏**：工程路径、Token 用量

## CLI 模式

`oiph cli` 进入交互式 REPL，支持 `/` 开头的本地指令：

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

## 架构

OIPH 为多 Agent 系统，与用户交互的是 supervisor，它以工具调用的形式调度子 Agent：

- **supervisor**：根据用户命令规划任务（原创 / 搬运流程）、调度子 Agent、检查质量、汇报
- **searching-agent**：负责搜索工作：搜索冷门题目与资料（std、测试数据、辅助程序），估计难度与知识点
- **statement-agent**：负责文字工作：出题 / 写题面 / 改编题面 / 写题解
- **solution-agent**：作为验题人，设计算法写 std 及其他解法，预估评测结果
- **auxiliary-agent**：写 generator / checker / validator / interactive_lib 并造数据

所有 Agent 支持工具调用、RAG 知识库、Skill 加载。
比赛以比赛工程的形式存储，Agent 可以调用专门的
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
- 在比赛目录下启动 `oiph` 时自动加载上一次的会话（按 `current` 指针，缺失则取最近修改的）
- 每轮对话后自动保存；`/session new|use` 切换前也会保存当前会话
- `oiph session export` 或 `/session export` 可导出为 markdown

## 导出

```sh
# 导出为 LemonLime 格式
./target/debug/oiph export lemon [输出目录]

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

- Polygon API
- tuack 和 tuack-ng 格式导出
- 其他 OJ 格式导出（SYZOJ 等）

## 查重（原题查找）

- `duplicate_check` 工具支持两个后端：**cpret.online**（默认）与 yuantiji.ac
- CLI `--dup-backend cpret|yuantiji` 设置默认后端；工具的 `backend` 参数可逐次覆盖
- 内置频率控制（≥3s 间隔），两个后端共享
