---
name: duipai
description: OI 题目对拍（随机数据压力测试）：用暴力解与被测程序在小规模随机数据上对比输出找反例。严格限制对拍组数与单次运行超时，防止无限运行。验证 std、检查解法预期时使用。
---

# OI 对拍

## 适用场景

- 验证新写的 std 是否正确：与 trusted 暴力解在小规模随机数据上对比输出。
- 验证某个解法在什么数据上会 WA/RE/TLE（用于设计部分分、确认错误解法的预期评测结果）。

对拍用的生成器、暴力程序、比对脚本**不要求** C++14/testlib（那是对最终 auxiliary
程序的要求），用 python3、shell、C++ 等最顺手的方式即可。这些是一次性脚本，不必放进
`auxiliary/`，建议放在题目目录下的 `duipai/`（可随时删除）。

## 硬性限制（必须遵守，防止无限运行）

1. **组数上限**：默认 200 组，最多不超过 1000 组；达到上限立即停止。
2. **单次运行限时**：运行每个程序都必须用 `timeout`（默认 5 秒）；超时或崩溃按一次
   不一致处理，记录现场后停止，绝不让对拍卡死。
3. **总时长**：整轮对拍控制在几分钟内。若已接近组数上限仍未找到反例，直接停止并汇报
   “未发现反例”。
4. **找到反例即停**：保存输入、两份输出到 `duipai/`，立即报告，不要继续跑。

## 工作流

1. 阅读题面（`statement/zh_cn.md`）与数据范围，确定“小规模参数”（例如
   $n \le 10$、值域 $[-5, 5]$），保证 trusted 暴力解在该范围内一定正确且足够快。
2. 准备两个程序：trusted（暴力/慢但正确）与被测程序（std 或待验证解法），分别编译。
3. 写随机数据生成器 gen：接受命令行参数（种子/规模），输出到 stdout。不必用 testlib。
4. 循环对拍（见下方模板）。输出不唯一的题（SPJ/多解/实数误差）改用 checker 比较，
   不要用 `diff`。
5. 汇报：跑了多少组、是否找到反例、反例文件位置、初步原因分析。

## 脚本模板（bash）

```bash
#!/usr/bin/env bash
set -u
MAX_GROUPS=200   # 组数上限
TIMEOUT=5        # 每次运行限时（秒）
mkdir -p duipai
for seed in $(seq 1 "$MAX_GROUPS"); do
  python3 gen.py "$seed" > duipai/in.txt
  if ! timeout "$TIMEOUT" ./sol < duipai/in.txt > duipai/out_sol.txt 2> duipai/err_sol.txt; then
    echo "被测程序异常 seed=$seed"; cp duipai/in.txt duipai/fail.in; break
  fi
  if ! timeout "$TIMEOUT" ./brute < duipai/in.txt > duipai/out_brute.txt 2> duipai/err_brute.txt; then
    echo "trusted 程序异常 seed=$seed"; cp duipai/in.txt duipai/fail.in; break
  fi
  if ! diff -q duipai/out_sol.txt duipai/out_brute.txt > /dev/null; then
    echo "找到反例 seed=$seed"; cp duipai/in.txt duipai/fail.in; break
  fi
done
```

python 版本：`subprocess.run(cmd, stdin=..., timeout=TIMEOUT)` 并捕获
`subprocess.TimeoutExpired`，逻辑同上。

## 注意事项

- 生成器要覆盖边界：$0$、$1$、极大值、重复元素、负数、全相同等极端情况用固定种子单独试。
- 随机数据要有梯度：先小后大，小数据最容易暴露逻辑错误。
- 交互题对拍前先写一个简单的模拟交互器，把交互转换为普通 IO。
- trusted 程序要写得显然正确（哪怕慢），不要复用被测算法的思路。
