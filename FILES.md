## 目录结构  

contest_name
    - config.yaml            # 比赛配置（名称、id、题目列表、创建时间）
    - .oiph/
        - kb/                # 工程知识库（kb.json）
        - skills/            # 工程 skills（<名>/SKILL.md）
        - sessions/          # 会话（<名>.json + current 指针文件）
    - problem_name_a
        - config.yaml        # 题目配置（名称、类型、来源、标签、时限、各组件状态）
        - statement
            - zh_cn.md       # 题面（Markdown + LaTeX）
            - tutorial.md    # 题解（可选，也写在 statement 下）
            - down
                - 下发文件（交互库等）
        - data
            - 1.in
            - 1.ans
            - ...            # 测试数据文件（生成的或静态的）
        - auxiliary
            - generator.cpp  # 数据生成器（始终在 auxiliary/ 下）
            - validator.cpp
            - checker.cpp
            - interactive_lib.cpp（如需要）
        - solutions
            - std.cpp        # 标准答案
            - <name>.cpp     # 其他解法（暴力、错误解法等）
    - problem_name_b
        - ...

## 全局数据（不在工程内）

- `~/.oiph/kb/`：全局知识库（仓库 `assets/kb/` 文档在启动时自动种子到此）
- `~/.oiph/skills/`：全局 skills（`<名>/SKILL.md`；仓库 `assets/skills/` 在启动时自动种子到此）
- 检索时合并全局与工程知识库；skills 同名时工程覆盖全局

## 配置文件结构

配置文件全部采用 YAML 格式。

### 比赛 config.yaml

- `id`：比赛唯一 id（UUID）
- `name`：比赛名称
- `problems`：题目目录名列表
- `config`：`start_time` / `duration_min` / `notes`（可选）
- `created_at`：创建时间

### 题目 config.yaml

- `id`：题目目录名
- `name`：题目名称
- `problem_type`：`traditional` | `interactive_lib` | `interactive_io` | `answer_only` | `function`
- `source`：`original`（原创）| `moved`（搬运）| `adapted`（改编）
- `tags`：知识点标签列表
- `time_limit_ms` / `memory_limit_mb` / `compile_flags`：评测参数
- `subtasks`：测试点配置列表（见下）
- `data_gen`：数据生成参数 map，key 为测试点名称，value 为 generator 命令行参数
- 各组件状态（见下）：`statement`、`std`、`sols`、`data`、`validator`、`checker`、`interactive_lib`、`tutorial`
- `duplicate_check`：查重结果（`found`/`matches`/`checked_at`/`note`）
- `files`：各组件相对路径

组件状态为带内部标签的枚举（实现 `GetStatus` trait，聚合规则：Failed > InProgress > Completed > NotStarted）：

```yaml
statement:
  state: not_started            # | in_progress { progress, message }
                                # | completed { timestamp }
                                # | failed { error }
```

`sols` 是列表，每项含 `name`、`file`、`expected`（预期评测结果 `verdict`/`score`）与状态：

```yaml
sols:
  - name: brute
    file: solutions/brute.cpp
    expected:
      verdict: WA
      score: 30.0
    status:
      state: not_started
```

### subtasks（在题目 config.yaml 中）

```yaml
subtasks:
  - score: 30        # 分数
    type: sum        # 子任务计分方式，包括 sum, min, mul
    cases: [1, 2]    # 测试点名称列表
    pretest: true    # 是否是 pretest，不写此字段则默认为 false
    sample: true     # 是否是样例，不写此字段则默认为 false，如果是样例则导出时会自动放进 statement/down 里
    depend: []       # 一个列表，表示依赖的子任务编号（从 1 开始），如果列表中任意一个子任务不是满分则此子任务自动记为 0 分
```

### data_gen（在题目 config.yaml 中）

数据生成参数 map。key 为测试点名称（subtasks.cases 中的项），value 为 generator 的命令行参数。
生成数据时，若测试点在 data_gen 的 key 中，执行 `<auxiliary/generator> <value>` 生成该测试点输入；
不在 data_gen 中的测试点视为已有静态数据文件。

```yaml
data_gen:
  "1": "-small"
  "2": "-big"
  "hack": "-hack"
```
