#!/usr/bin/env bash
# 兼容入口：init 功能已集成到主程序（oiph init）。
# 用法保持不变：./init.sh [ASSETS_DIR] [--force]
#   - ASSETS_DIR 缺省为脚本所在目录旁的 assets/
#   - --force 强制覆盖已存在的 skills / prompts / vendor
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 检测 oiph 二进制（兼容旧名 preparer）
BIN=""
for alt in "$SCRIPT_DIR/target/debug/oiph" "$SCRIPT_DIR/target/release/oiph" \
           "$(command -v oiph 2>/dev/null || true)" \
           "$SCRIPT_DIR/target/debug/preparer" "$SCRIPT_DIR/target/release/preparer" \
           "$(command -v preparer 2>/dev/null || true)"; do
    if [ -n "$alt" ] && [ -x "$alt" ]; then BIN="$alt"; break; fi
done
if [ -z "$BIN" ]; then
    echo "错误：找不到 oiph 可执行文件（先 cargo build）" >&2
    exit 1
fi

ASSETS_DIR="$SCRIPT_DIR/assets"
FORCE_FLAG=""
for arg in "$@"; do
    case "$arg" in
        --force) FORCE_FLAG="--force" ;;
        *) ASSETS_DIR="$arg" ;;
    esac
done

exec "$BIN" init $FORCE_FLAG --assets "$ASSETS_DIR"
