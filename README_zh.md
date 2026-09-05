# OIPH - 信息学竞赛组题助手 Agent

OIPH（OI Preparer Helper）是一个专为辅助信息学竞赛模拟赛组题工作全流程而设计的 Agent 工具，工作流涵盖找题/出题、造数据、写题解、验题、集成测试、打包等。

## 从源码构建

### 工具链准备
本项目主体使用 Rust 编写，前端用 Vite+React+TypeScript 构建，需要安装 Rust（1.85+）和 Node.js（18+） 工具链。参见 [安装 Rust](https://rust-lang.org/zh-CN/tools/install/) 和 [下载 Node.js®](https://nodejs.org/zh-cn/download) 或按以下步骤安装：

Rust：  
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh      # 安装 Rust 工具链
cargo --version     # 查看 Rust 工具链版本
```

Node.js：  
```sh
# 下载并安装 nvm
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash

# 代替重启 shell
\. "$HOME/.nvm/nvm.sh"

# 下载并安装 Node.js：
nvm install 24
# 验证 Node.js 版本：
node -v # Should print "v24.20.0".
# 验证 npm 版本：
npm -v # Should print "11.19.0".
```

### 构建 OIPH

```sh
git clone https://github.com/sxrzh/oiph.git && cd oiph

# 构建前端，产出 frontend/dist
cd frontend
npm install
npm run build
cd ..

# 构建 Rust 二进制，产出 target/release/oiph
cargo build --release

# 安装到系统目录
sudo cp target/release/oiph /usr/local/bin/

# 初始化 oiph 配置并安装前端
oiph init
# 如果之前安装过 oiph，则用 oiph init --force 重置配置

# 验证安装
mkdir test && cd test
oiph
```

## 使用方法  

本项目采用 Web 界面，在浏览器中与用户交互。

使用时先 `cd` 到比赛工程目录下，之后运行
```sh
oiph
```
浏览器访问 <http://localhost:17217> 即可打开界面。

### 指定 GUI 端口
```sh
oiph --port 8080
```

### 子命令
```sh
oiph help          # 显示帮助
oiph status        # 查看当前工程状态
oiph test [题目id]  # 集成测试，不指定题目 id 则测试整个比赛
oiph export lemon  # 导出比赛为 LemonLime 兼容格式

# 系统提示词管理
# 以下 AGENT 参数可以是 supervisor|statement|searching|auxiliary|solution|compactor
oiph prompt update <AGENT> <FILE>    # 用文件替换某个 Agent 的系统提示词
oiph prompt edit <AGENT> [EDITOR]    # 用编辑器（默认 vim）编辑某个 Agent 的系统提示词

oiph cli           # 简易的 CLI 界面，功能较少，建议使用 GUI 界面
```

```sh
# agent 配置（~/.oiph/config/agents.json：每个 agent 的 base_url / api_key / prompt，
# 以及可选的 reasoning（思考模式）、price（固定单价 {input,hit,output,currency}，货币/M token）、
# price-policy: "auto"（按 base_url 自动识别供应商计价，支持 DeepSeek 峰谷计价，
# GLM 不估算费用；price 与 price-policy 都缺省时即 auto）、
# max_context（最长上下文 token 估算，超过则自动调用 compactor 压缩，默认 1048576）。
# 另有可选的 "compactor" 项指定上下文压缩模型与提示词，缺省回退 supervisor）
# 提示词编辑（存于 ~/.oiph/config/prompts/<agent>.md）
oiph prompt update statement new_prompt.md  # 从文件替换
oiph prompt edit statement                  # vim 编辑（git commit 风格）
oiph prompt edit solution code              # 指定编辑器
```

### 知识库管理（两级）
- 全局知识库：`~/.oiph/kb/`；工程知识库：`<比赛工程>/.oiph/kb/`（检索时两者合并）
- `init.sh` 把 `assets/kb/` 构建到全局知识库（来源标签为 `<builtin>/...`）
- `kb add` 默认加到工程知识库（无比赛工程则全局），`-g/--global` 强制全局

```sh
oiph kb add assets/kb/statement_req.md
oiph kb add some_doc.txt -g
oiph kb list
oiph kb clear        # 默认清工程，不存在则清全局
oiph kb clear -g
oiph kb search "题面规范 全角标点"
```

### skills 管理（两级：全局 ~/.oiph/skills，工程 <比赛工程>/.oiph/skills）
```sh
oiph skill list
oiph skill show duipai
oiph skill add my_skill.md mine      # 安装到工程（无比赛则全局）
oiph skill add my_skill.md mine -g   # 安装到全局
oiph skill delete mine
```

### 会话管理（数据存于 <比赛工程>/.oiph/sessions/）
```sh
oiph session list
oiph session new          # 自动命名
oiph session new myname
oiph session use myname
oiph session delete old
oiph session export        # 导出当前会话为 markdown
oiph session show myname   # 打印会话内容
```

## 功能介绍  

### GUI 界面

GUI 界面用户友好，能实时查看 agent 修改的内容。
- **左侧题目区**：题目选项卡 + 基本信息/题面/题解/数据/辅助程序/解法标签页
- **右侧对话区**：与 supervisor agent 实时流式对话（含思维链）、session 切换、中止/undo/redo 按钮
- **顶部菜单**：导出、集成测试、设置
- **底部状态栏**：工程路径、Token 用量

### 知识库  

知识库是 agent 运行过程中可以通过 RAG 查询的文本文档数据库。  
OIPH 内置了 OI 题目格式规范、OIPH 比赛项目结构、OI 冷门题目来源、testlib 文档等定制知识库。

### Skills

Skill 主要用于在需要时加载特定知识，在系统提示词中只会嵌入 skill 的简介，在 agent 需要某个 skill 时才会将 skill 全文注入上下文。OIPH 中供 searching-agent 调用的不同 OJ API 都是通过 skill 的形式实现的。  

Skill 是一个目录，内含 `SKILL.md`（YAML frontmatter：`name`、`description`，后接指令正文）：

```yaml
---
name: duipai
description: OI 题目对拍：用暴力解与被测程序对比找反例，限制组数与运行时间。
---
# 正文指令
```
此外目录中也可以包含 `scripts` 等辅助性内容。

OIPH 中 skill 有两个来源：
- 全局 skills：`~/.oiph/skills/`；
- 工程 skills：`<比赛工程>/.oiph/skills/`

Agent 实际能调用的 skills 是这两个来源的并集（同名时工程覆盖全局）。  
OIPH 安装时内置了定制化的 OI 相关 Skill（如对拍）和冷门 OJ API Skill（目前实现了 CodeChef API）

### 会话（Session）

- supervisor 对话自动保存为 JSON，存于 `<比赛工程>/.oiph/sessions/<名>.json`
- 在比赛目录下启动 `oiph` 时自动加载上一次的会话（按 `current` 指针，缺失则取最近修改的）
- 每轮对话后自动保存；`/session new|use` 切换前也会保存当前会话
- `oiph session export` 或 `/session export` 可导出为 markdown
- OIPH 的 session 中每条对话是与工作区状态相关联的，在 Web 界面中执行 undo/redo 不仅会撤销/重做对话，也会将比赛工程恢复到对应对话后的快照。

### 导出

```sh
# 导出为 LemonLime 格式
oiph export lemon [输出目录]

# 默认输出到 <比赛目录>/<比赛名>_lemon/
```

LemonLime 导出内容包括：
- `<比赛名>.cdf`：比赛配置 JSON（含 subtasks、依赖、SPJ 配置等）
- `data/<题目id>/`：测试数据文件（.in/.ans）
- `data/<题目id>/spj.cpp` + `testlib.h`：SPJ（checker）使用 LemonLime 兼容的 testlib.h
- `data/<题目id>/grader.cpp` + `<题目id>.h`：交互题 grader 与交互库头文件
- `compile_spj.bat`：编译所有 SPJ 的批处理脚本（Windows g++）


## 架构

OIPH 为多 Agent 系统，与用户交互的是 supervisor，它以工具调用的形式调度子 Agent：

- **supervisor**：根据用户命令规划任务（原创 / 搬运流程）、调度子 Agent、检查质量、汇报
- **searching-agent**：负责搜索工作：搜索冷门题目与资料（std、测试数据、辅助程序），估计难度与知识点
- **statement-agent**：负责文字工作：出题 / 写题面 / 改编题面 / 写题解
- **solution-agent**：作为验题人，设计算法写 std 及其他解法，预估评测结果
- **auxiliary-agent**：写 generator / checker / validator / interactive_lib 并造数据

所有 Agent 支持工具调用、RAG 知识库、Skill 加载。
比赛以比赛工程的形式存储，Agent 可以调用专门定制的工具管理比赛工程中的题目编排、为一个题目注册多个不同解法、对题目和比赛进行集成测试等。
本项目为 OI 组题定制了查重工具 `duplicate_check`，支持两个查重后端：[CPRet](https://cpret.online)（默认）与 [yuantiji](https://yuantiji.ac)。  

【架构图】

本 Agent 的工作流针对 OI 组题定制，主要分为 原创 和 搬运/改编 两种路线：
- 原创：【流程图】
- 搬运/改编：【流程图】

除函数式交互题的交互库外，辅助程序均强制使用 testlib，力求使比赛工程更加现代化。

### 工程目录与配置

见 FILES.md。比赛与题目的数据存本地文件，配置存对应目录的 `config.yaml`；组件状态实现 `GetStatus` trait（见 src/model.rs）。

## Coming Soon

- 更多冷门题目来源 OJ 的 API
- Polygon API
- tuack 和 tuack-ng 格式导出
- OJ 格式导出（HydroOJ, SYZOJ 等）
- 知识库目前用 JSON 直接存储，未来将改为 Redis

## 已知问题  

- **安全问题**：目前没有实现沙箱、权限控制，agent 的工具可以访问到整个系统的文件，而且有可能运行从网络上抓取的代码。建议您在虚拟机或 docker 中运行 OIPH 并严格监控防止信息泄露；
- 提交答案题的功能目前还没有得到充分测试。