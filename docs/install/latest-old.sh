#!/usr/bin/env bash
#
# This is deprecated, just for archive.
#
# ============================================================================
# OIPH 一键安装脚本（从 GitHub Release 安装最新版）
#
# 用法：
#   curl -fsSL https://raw.githubusercontent.com/sxrzh/oiph/main/docs/install/latest.sh | bash
#   （或先下载后执行：bash latest.sh）
#
# 功能：
#   1. 检测本机架构，选择对应的发布包（Linux 为 musl 静态链接，无 glibc 依赖）
#   2. 若已安装（/usr/local/bin/oiph 或 ~/.oiph 存在）则询问是否继续
#   3. 通过 GitHub API 获取最新 release，下载合适架构的 zip 并解压
#   4. sudo cp 安装 oiph 到 /usr/local/bin
#   5. 运行 oiph init --force 安装配置（skills/kb/prompts/vendor/前端）
#
# 说明：init --force 会覆盖 ~/.oiph 中已有的 skills / prompts / vendor 与前端，
#       但不会覆盖 agents.json 与 limit.json（请自行在设置页或文件中配置模型）。
# ============================================================================

set -euo pipefail

REPO="sxrzh/oiph"
# 安装目录（可用环境变量覆盖，便于测试/自定义安装位置）
INSTALL_DIR="${OIPH_INSTALL_DIR:-/usr/local/bin}"

c_green='\033[1;32m'; c_cyan='\033[1;36m'; c_yellow='\033[1;33m'; c_red='\033[1;31m'; c_off='\033[0m'
say() { printf "${c_cyan}[oiph]${c_off} %s\n" "$*"; }
ok()  { printf "${c_green}[oiph]${c_off} %s\n" "$*"; }
warn(){ printf "${c_yellow}[oiph] 警告：${c_off}%s\n" "$*" >&2; }
die() { printf "${c_red}[oiph] 错误：${c_off}%s\n" "$*" >&2; exit 1; }

# ---------------------------------------------------------------------------
# 1. 检测架构并选择发布包
# ---------------------------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS-$ARCH" in
  Linux-x86_64 | Linux-amd64)        ASSET="oiph-x86_64-unknown-linux-musl.zip" ;;
  Linux-aarch64 | Linux-arm64)       ASSET="oiph-aarch64-unknown-linux-musl.zip" ;;
  Darwin-arm64 | Darwin-aarch64)     ASSET="oiph-aarch64-apple-darwin.zip" ;;
  Darwin-x86_64 | Darwin-i386)       die "暂不支持 Intel Mac（没有对应发布包），请从源码构建：git clone https://github.com/$REPO" ;;
  *) die "不支持的系统/架构：$OS $ARCH" ;;
esac
say "检测到平台：$OS $ARCH → 安装包：$ASSET"

# ---------------------------------------------------------------------------
# 2. 已安装则询问
# ---------------------------------------------------------------------------
if [ -x "$INSTALL_DIR/oiph" ] || [ -d "$HOME/.oiph" ]; then
  warn "检测到本机已安装 oiph（$INSTALL_DIR/oiph 或 ~/.oiph 已存在）。"
  warn "继续将：覆盖 $INSTALL_DIR/oiph 二进制，并以 --force 重置 ~/.oiph（skills/prompts/vendor/前端）。"
  read -r -p "是否继续？[y/N] " ans
  case "$ans" in
    y | Y | yes | YES) ;;
    *) say "已取消。"; exit 0 ;;
  esac
fi

# ---------------------------------------------------------------------------
# 3. 获取最新 release 与下载地址
# ---------------------------------------------------------------------------
say "正在获取最新版本信息（api.github.com/repos/$REPO/releases/latest）…"
API_JSON="$(curl -fsSL --retry 2 "https://api.github.com/repos/$REPO/releases/latest")" \
  || die "获取 release 信息失败（网络不可达或 API 限流？）。可稍后重试。"
TAG="$(printf '%s' "$API_JSON" | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
# 与布局无关地解析：列出所有 browser_download_url，按包名精确匹配
# （不能依赖 name 与 download_url 的行距——真实 API 的 uploader 对象很大）
URL="$(printf '%s' "$API_JSON" \
        | sed -n 's/^[[:space:]]*"browser_download_url":[[:space:]]*"\([^"]*\)".*/\1/p' \
        | grep -F "/$ASSET" \
        | head -n1)"
[ -n "$TAG" ] || die "解析最新版本号失败"
if [ -z "$URL" ]; then
  die "最新版本 $TAG 中没有找到安装包 $ASSET（可能该版本未构建此架构）。"
fi
say "最新版本：$TAG"

# ---------------------------------------------------------------------------
# 4. 下载并解压到临时目录
# ---------------------------------------------------------------------------
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
say "下载 $ASSET …"
curl -fSL --retry 2 "$URL" -o "$TMP/$ASSET" || die "下载失败"

cd "$TMP"
if command -v unzip >/dev/null 2>&1; then
  unzip -q "$ASSET" || die "解压失败"
elif command -v python3 >/dev/null 2>&1; then
  python3 -m zipfile -e "$ASSET" . || die "解压失败"
elif tar --help 2>&1 | grep -q bsdtar; then
  tar -xf "$ASSET" || die "解压失败"
else
  die "缺少 unzip，请先安装（apt install unzip / brew install unzip）"
fi
[ -f oiph ] || die "安装包内容不完整（缺少 oiph 可执行文件）"
[ -d assets ] || warn "安装包缺少 assets 目录，oiph init 可能无法安装内置内容"

# ---------------------------------------------------------------------------
# 5. 安装二进制到 /usr/local/bin
# ---------------------------------------------------------------------------
SUDO=""
[ "$(id -u)" -eq 0 ] || SUDO="sudo"
say "安装到 $INSTALL_DIR/oiph（$SUDO cp）…"
$SUDO cp -f oiph "$INSTALL_DIR/oiph"
$SUDO chmod 755 "$INSTALL_DIR/oiph"

# ---------------------------------------------------------------------------
# 6. 初始化配置（在当前解压目录执行，便于发现 ./assets 与 ./frontend/dist）
# ---------------------------------------------------------------------------
say "运行 oiph init --force 安装配置（skills / kb / prompts / vendor / 前端）…"
"$INSTALL_DIR/oiph" init --force || die "oiph init 失败"

# ---------------------------------------------------------------------------
ok "安装完成！"
echo
echo "  启动 Web 界面：        oiph            （浏览器访问 http://localhost:17217）"
echo "  命令行交互：           oiph cli"
echo "  查看帮助：             oiph --help / oiph --version"
echo
echo "  下一步：在 Web 界面的『设置』中配置每个 agent 的 Base URL / API Key / 模型，"
echo "  或设置环境变量 OPENAI_BASE_URL / OPENAI_API_KEY / OPENAI_MODEL。"
echo "  安装脚本不会改动 agents.json 与 limit.json（预算），可随时用 oiph fee reset 重置预算。"
