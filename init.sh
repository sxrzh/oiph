#!/usr/bin/env bash
# init.sh：安装 OIPH 默认的提示词、skills 和知识库到 ~/.oiph
#
# 用法：
#   ./init.sh [ASSETS_DIR] [--force]
#   ASSETS_DIR  来源目录（需含 kb/ 和 skills/ 子目录），默认为脚本所在目录
#
# 示例：
#   ./init.sh                          # 用脚本所在目录的 assets/
#   ./init.sh /opt/oiph-assets         # 用指定目录（其下需有 kb/、skills/）
#   ./init.sh --force                  # 强制覆盖已存在的 skills
#
# - 检测 ~/.oiph，不存在则创建
# - 复制 skills/* 到 ~/.oiph/skills/（整个目录，不只 SKILL.md）
# - 递归遍历 kb/ 下所有文件，构建全局知识库 ~/.oiph/kb（kb add -g）

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OIPH_HOME="${HOME}/.oiph"
KB_DIR="${OIPH_HOME}/kb"
SKILLS_DIR="${OIPH_HOME}/skills"
CONFIG_DIR="${OIPH_HOME}/config"
PROMPTS_DIR="${CONFIG_DIR}/prompts"

# 解析参数：第一个非 --force 参数为 assets 目录，默认脚本所在目录
ASSETS_DIR=""
FORCE=0
for arg in "$@"; do
    case "$arg" in
        --force) FORCE=1 ;;
        -h|--help)
            grep '^#' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            if [ -z "$ASSETS_DIR" ]; then
                ASSETS_DIR="$arg"
            else
                echo "错误：多余的参数 '$arg'" >&2
                exit 1
            fi
            ;;
    esac
done
[ -z "$ASSETS_DIR" ] && ASSETS_DIR="$SCRIPT_DIR/assets"
ASSETS_DIR="$(cd "$ASSETS_DIR" && pwd)"

KB_ASSETS="$ASSETS_DIR/kb"
SKILLS_ASSETS="$ASSETS_DIR/skills"

# 检测 preparer 二进制
BIN=""
for alt in "$SCRIPT_DIR/target/debug/preparer" "$SCRIPT_DIR/target/release/preparer" "$(command -v preparer 2>/dev/null || true)"; do
    if [ -x "$alt" ]; then BIN="$alt"; break; fi
done
if [ -z "$BIN" ]; then
    echo "错误：找不到 preparer 可执行文件（先安装 oiph）" >&2
    exit 1
fi

# 1. 创建 ~/.oiph
mkdir -p "$OIPH_HOME"
echo "✓ $OIPH_HOME（来源：$ASSETS_DIR）"

# 2. 复制 skills（整个目录，含 SKILL.md 之外的其他文件）
if [ -d "$SKILLS_ASSETS" ]; then
    for skill in "$SKILLS_ASSETS"/*/; do
        name="$(basename "$skill")"
        if [ -d "$SKILLS_DIR/$name" ] && [ "$FORCE" -eq 0 ]; then
            echo "  skill $name 已存在，跳过（--force 覆盖）"
            continue
        fi
        mkdir -p "$SKILLS_DIR"
        rm -rf "$SKILLS_DIR/$name"
        cp -r "$skill" "$SKILLS_DIR/"
        echo "✓ 安装 skill $name"
    done
else
    echo "警告：$SKILLS_ASSETS 不存在" >&2
fi

# 3. 构建知识库（kb add -g 写入 ~/.oiph/kb，本地哈希 embedding 无需 API key）
# 递归遍历 kb 下所有文件（含子目录），source 标签保留相对路径：
#   <builtin>/statement_req.md、<builtin>/testlib/checkers/wcmp.cpp 等
if [ -d "$KB_ASSETS" ]; then
    mkdir -p "$KB_DIR"
    while IFS= read -r -d '' doc; do
        rel="${doc#"$KB_ASSETS"/}"
        # 来源标签用 <builtin>/<相对路径>，不暴露源码路径；重复添加按 source 去重覆盖
        if "$BIN" kb add -g --source "<builtin>/$rel" "$doc" >/dev/null 2>&1; then
            echo "✓ 知识库文档 $rel"
        else
            echo "⚠ 知识库文档 $rel 添加失败" >&2
        fi
    done < <(find "$KB_ASSETS" -type f -print0)
else
    echo "警告：$KB_ASSETS 不存在" >&2
fi

# 4. 安装默认提示词（已存在跳过，--force 覆盖）
PROMPTS_ASSETS="$ASSETS_DIR/prompts"
if [ -d "$PROMPTS_ASSETS" ]; then
    mkdir -p "$PROMPTS_DIR"
    for f in "$PROMPTS_ASSETS"/*.md; do
        [ -f "$f" ] || continue
        name="$(basename "$f")"
        if [ -f "$PROMPTS_DIR/$name" ] && [ "$FORCE" -eq 0 ]; then
            echo "  prompt $name 已存在，跳过（--force 覆盖）"
            continue
        fi
        cp "$f" "$PROMPTS_DIR/"
        echo "✓ 安装 prompt $name"
    done
else
    echo "警告：$PROMPTS_ASSETS 不存在" >&2
fi

# 5. 生成 agents.json（已存在则不动）
if [ ! -f "$CONFIG_DIR/agents.json" ]; then
    mkdir -p "$CONFIG_DIR"
    cat > "$CONFIG_DIR/agents.json" <<EOF
{
  "supervisor": { "base_url": null, "api_key": null, "prompt": "$PROMPTS_DIR/supervisor.md" },
  "statement":  { "base_url": null, "api_key": null, "prompt": "$PROMPTS_DIR/statement.md" },
  "solution":   { "base_url": null, "api_key": null, "prompt": "$PROMPTS_DIR/solution.md" },
  "auxiliary":  { "base_url": null, "api_key": null, "prompt": "$PROMPTS_DIR/auxiliary.md" },
  "searching":  { "base_url": null, "api_key": null, "prompt": "$PROMPTS_DIR/searching.md" }
}
EOF
    echo "✓ 生成 $CONFIG_DIR/agents.json"
else
    echo "  agents.json 已存在，跳过"
fi

echo "初始化完成。全局配置目录：$OIPH_HOME"
