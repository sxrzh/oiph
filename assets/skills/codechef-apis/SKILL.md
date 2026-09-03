---
name: codechef-apis
description: Query CodeChef practice problems and public annotated submissions. Use when the user wants to find CodeChef problems by difficulty/tags/keyword, read a problem statement, or study public accepted solutions with author-written line-by-line explanations.
---

# CodeChef 题目与提交记录 API

## 背景知识

CodeChef（codechef.com）有大量 OI 练习题目：

- **难度**：每道题有难度分 `difficulty_rating`，范围 0~5001；`-1` 表示未评定难度（老题常见）。CodeChef 难度 2500 分大约相当于 提高/省选- 难度，没有严格对应关系。
- **标签**：每道题带有若干标签。标签列表用 `get_tags.py` 获取，每个标签有 `tagName`（显示名）和 `tagSlug`（查询名）。**查询题目时必须使用 `tagSlug`（如 `data-structures`），不要用 `tagName`（如 `Data Structures`）**。
- **公开提交**：每道题有一些公开的 AC 提交记录，其中很多带有代码作者写的**逐行注解（explanation）**，是学习解法的好材料。
- 相关网页：题目页 `https://www.codechef.com/problems/<problem_code>`，提交页 `https://www.codechef.com/viewsolution/<submission_id>`，但是直接读这些网页 token 消耗量非常大，非必要**不要**直接读这些网页。

## 环境准备

**路径约定**：本文件中的所有相对路径均相对于本 skill 根目录（即本 SKILL.md 所在目录）。脚本对自身依赖（`_common.py`、`_vendor/`）按脚本所在位置解析，因此用绝对路径在任意工作目录下运行均可，例如 `python3 <本skill根目录>/scripts/get_tags.py`；只有安装依赖时需要定位到 `scripts/requirements.txt` 的实际路径。

只需 Python 3.8+。推荐安装依赖（更稳健）：

```bash
pip install -r scripts/requirements.txt   # requests, python-toon
```

说明：未安装 `requests` 时自动回退到标准库 urllib；未安装 `python-toon` 时自动使用 `scripts/_vendor/` 内置副本，TOON 输出始终可用。

## 脚本一览

所有脚本位于 `scripts/` 下，直接用 `python3 scripts/<脚本>.py` 运行。除 `get_sub_code.py` 外均支持 `--format JSON|TOON`：

- `JSON`：单行压缩 JSON（无多余空格/换行，默认）。
- `TOON`：TOON 格式，数组用表格对齐表示，比 JSON 省 30-60% token，推荐给 LLM 阅读时使用。

出错时输出 `error: ...` 到 stderr 并以非零码退出（如 404 = 题目/提交不存在或未公开）。

| 脚本 | 用途 |
|---|---|
| `get_tags.py` | 获取所有题目标签 |
| `list_prob.py` | 按难度/标签/关键词筛选题目并按难度排序 |
| `get_prob_content.py` | 获取题目内容（题面、样例、限制、时限、标签） |
| `list_explained_subs.py` | 列出题目公开的带注解 AC 提交 |
| `get_sub_code.py` | 获取某个公开提交的源代码 |
| `get_sub_explanation.py` | 获取某个提交代码的作者逐行解释 |

## 各脚本详情

### get_tags.py — 获取所有标签

```bash
python3 scripts/get_tags.py [--format TOON]
```

无参数。返回所有标签数组（已剔除 `problemCount` 为 0 的标签），每个元素含 `tagSlug`、`tagName`、`problemCount`。

### list_prob.py — 筛选题目

```bash
python3 scripts/list_prob.py [--probs-per-page 20] [--page-index 0] \
    [--sort-order asc|desc] [--search 关键词] \
    [--start-rating 0] [--end-rating 5001] [--tags slug1,slug2] \
    [--include-unrated] [--format TOON]
```

- `--probs-per-page`：每页最大题目数（API 的 `limit`），默认 20。
- `--page-index`：页码（API 的 `page`），**从 0 开始**，默认 0。
- `--sort-order`：按难度升序 `asc`（默认）或降序 `desc`。
- `--search`：关键词，在题目名称和题目 code 中匹配。
- `--start-rating` / `--end-rating`：难度范围，默认 0~5001。
- `--tags`：逗号分隔的标签，**传入 tagSlug**，可以通过 list_tags.py 获取支持的所有 tag；脚本会自动标准化（转小写、空白转 `-`），所以 `--tags "Dynamic Programming"` 也可以；CodeChef 搜索时采取“或”的规则：题目包含查询的任意一个标签就会返回到结果中。
- `--include-unrated`：保留难度未评定（`difficulty_rating=-1`）的题目；默认总是过滤掉它们（除非显式加上该选项）。

输出 `data` 数组全文（已剔除 `id`、`intended_contest_id`、`actual_intended_contests`、`contest_code` 字段）：`code`（题目 code）、`name`、`difficulty_rating`、提交统计等。结果不足一页说明已到末尾。

**注意**：`--tags` 中若包含无效的 tagSlug，API 会返回 HTTP 410；不确定时先用 `get_tags.py` 查证。API 的难度范围查询可能混入未评定难度（-1）的题目（计数含它们），脚本默认已将其剔除，因此单页返回条数可能少于 `--probs-per-page`。

### get_prob_content.py — 获取题目内容

```bash
python3 scripts/get_prob_content.py <problem_code> [--format TOON]
```

`problem_code` 为题目 code（如 `TRIANGLE7`）。输出对象仅保留以下字段：

- `problem_code`、`problem_name`
- `body`：题面全文（markdown + LaTeX）。若 body 是 CodeChef 的占位模板（无实际题面），则**省略该字段**，此时真正题面在 `problemComponents.statement` 中。
- `problemComponents`：题面组件，含 `statement`（题面）、`inputFormat`、`outputFormat`、`sampleTestCases`（样例输入输出及解释）、`constraints`（约束）、`subtasks`（子任务）。
- `max_timelimit`：时限，**已乘 1000，单位 ms**。
- `difficulty_rating`：难度。
- `best_tag`、`user_tags`、`computed_tags`：仅在有且非空时输出。

### list_explained_subs.py — 列出公开的带注解提交

```bash
python3 scripts/list_explained_subs.py <problem_code> [--format TOON]
```

输出该题公开的 AC 提交（`result_code=1`）中带注解的部分，每个元素只含 `submission_id`、`language`。

### get_sub_code.py — 获取提交代码

```bash
python3 scripts/get_sub_code.py <submission_id>
```

不支持 `--format`。直接输出代码正文（已删除所有 `\r`，换行为 `\n`），便于保存为文件阅读/编译。

### get_sub_explanation.py — 获取代码作者的解释

```bash
python3 scripts/get_sub_explanation.py <submission_id> [--format TOON]
```

输出 `annotations` 数组，每个元素只含 `from_line`、`to_line`、`annotation`。`from_line`/`to_line` 是 `get_sub_code.py` 输出代码的**1-based 行号区间**，`annotation` 是作者对这段代码的解释（markdown）。

## 推荐工作流

1. **找题**：`get_tags.py` 拿到 tagSlug → `list_prob.py --tags <slug> --start-rating X --end-rating Y` 分页浏览 → 对感兴趣的题目 `get_prob_content.py <code>` 读题面和样例。
2. **学习解法**：`get_prob_content.py` 读题 → `list_explained_subs.py <code>` 找带注解的 AC 提交 → `get_sub_code.py <id>` 看代码 → `get_sub_explanation.py <id>` 对照行号阅读作者解释。
3. **批量分析**：用 `--format TOON` 减少输出 token；只需代码本身时用 `get_sub_code.py` 保存到临时文件再阅读。

## 注意事项

- 请按需、串行地调用接口，不要并发大量请求。
- `list_prob.py` 的 `--search` 是模糊匹配，大小写不敏感。
- 提交必须公开（题目页展示的带注解 AC 提交）才能取到代码；未公开时脚本报错。
- LaTeX/markdown 原样保留，阅读题面时注意 `$...$` 是数学表达式。
