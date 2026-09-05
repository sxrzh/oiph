# 评测方式说明 / judging.md

本文描述本工具的评测方式，**与集成测试（integrity test）完全一致**。组题时编写 std、sols、auxiliary 程序应以此为准。

## 总览

题目类型四种：`traditional`（传统题）、`function`（函数交互题）、`interactive_io`（IO 交互题）、`answer_only`（提交答案题）。

- 辅助程序一律放在题目目录 `auxiliary/` 下：`generator.cpp`、`validator.cpp`、`checker.cpp` 必需，交互题另有 `interactive_lib.cpp`（函数交互题还有 `<题目ID>.h`），除函数交互题的 `interactive_lib.cpp` 外，所有辅助程序**必须**基于 testlib。
- 编译命令：`g++ <compile_flags> -I auxiliary -o <输出> <源文件>`；`testlib.h` 已由题目创建时放入 `auxiliary/`（来自 `~/.oiph/vendor/testlib.h`）。
- generator / validator / checker 缺失任何一个，该题集成测试直接报错终止。
- 所有子进程都有超时保护；时间上限 `T = ceil(time_limit_ms × 1.5 / 1000)` 秒。

## 数据准备（四种题型一致）

1. 测试点列表来自 config.yaml 的 `subtasks.cases`；为空则自动发现 `data/*.in`。两者都没有 → 仅给出警告，跳过后续步骤。
2. 每个测试点的输入：
   - `data_gen` 中有该测试点：运行 `generator <参数>`，**stdout 重定向**写入 `<case>.in`（60s 超时）；
   - 否则复制 `data/<case>.in`；两者都没有 → 报错。
3. 输入校验：`timeout 30 ./validator < <case>.in`（**stdin 重定向，不是命令行参数**）。validator 用 testlib `registerValidation` 初始化，退出码 0 表示合法，非 0 视为校验失败。

## 传统题（traditional）

- std 单独编译为可执行文件。
- 生成答案：`timeout T ./std < <i>.in > <i>.ans`（stderr 捕获）。退出码 124 → TLE；非 0 → RE；成功后 std 用时超过原始时限（非 1.5 倍）会给警告。
- std 答案自检：`checker <i>.in <i>.ans <i>.ans`（同一文件既是选手输出又是标准输出，30s 超时）。
- 各 sols 同样单独编译、按测试点运行（输入重定向 `<i>.in`，输出到临时文件），用 checker 判定。

## 函数交互题（function）

- `auxiliary/<题目ID>.h` 是交互接口声明，`auxiliary/interactive_lib.cpp` 是交互实现。
- 编译：把待测源文件、`interactive_lib.cpp`、auxiliary 下全部 `.h` 放到同一目录，然后
  `g++ <compile_flags> -I . -o <输出> <待测>.cpp interactive_lib.cpp`。
- **运行与传统题完全相同**（stdin 重定向 `<i>.in`，stdout 写输出文件）——交互逻辑在编译期已内嵌进可执行文件。
- std 与所有 sols 都按此方式联合编译。

## IO 交互题（interactive_io）

- 待测程序（std 或 sol）**单独**编译；`interactive_lib.cpp` 单独编译为 **grader**。
- grader 约定：`argv[1]` = 输入文件路径，`argv[2]` = 输出文件路径（最终答案写入此文件），stdin/stdout 与待测程序交互。grader **必须**基于 testlib interactor（`registerInteraction`）。
- 每个测试点通过命名管道相连，等价于以下 bash 脚本：

  ```sh
  mkfifo pipe_in pipe_out
  timeout T ./<待测程序> < pipe_in | tee pipe_out &
  timeout T ./grader "<i>.in" "<i>.out" < pipe_out | tee pipe_in
  echo ${PIPESTATUS[0]} > grader_rc   # 精确捕获 grader 自身退出码
  wait
  rm -f pipe_in pipe_out
  ```

- 判定要点：
  - **grader 退出码必须为 0**，否则记 RE（用 `PIPESTATUS[0]` 取，不受 tee 干扰）；
  - 整体被 timeout 杀死（124）→ TLE；
  - 正常结束后用 `checker <i>.in <i>.out <i>.ans` 判定。
- 生成标准答案时，待测程序就是 std，grader 把答案写入 `<i>.ans`。

## 提交答案题（answer_only）

- **没有 std**，不编译、不运行任何程序生成答案。
- `data/` 必须同时提供 `<i>.in` 和 `<i>.ans`（标准输出）；缺 `.ans` 会报错。
- 每个 sol 是一个**目录**（config.yaml 中 `sols[].file` 指定，缺省 `solutions/<sol名>/`），目录内存放各测试点的输出文件：优先 `<case>.out`，缺失时回退 `<case>.ans`；某测试点没有输出文件 → 该点 WA。
- 判定：`checker <i>.in <sol输出> <data的i.ans>`。

## checker 约定（所有题型共用）

- 基于 testlib：`registerTestlibCmd(argc, argv)`；参数顺序固定：**输入文件、选手输出、标准答案**。
- 退出码 0 → AC；非 0 → WA（按 _fail 类型给出）。checker 运行失败（无法启动等）记 RE。
- checker.cpp 缺失时集成测试直接报错——不存在"无 checker 用 diff"的回退。

## 结果与预期比对

- 每个 sol 在 config.yaml 中有预期结果（`expected.verdict`）：
  - 预期 **AC**：要求所有测试点都 AC，否则警告；
  - 预期 **非 AC**（WA/RE/TLE 等）：任一测试点出现该结果即符合预期，全部不符则警告。
- 无 checker 时 `judge_output` 防御性记 RE（正常流程不会走到，前置已报错终止）。
