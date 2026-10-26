#!/usr/bin/env bash
#
# FinkMeter macOS 一键安装脚本
#
# 用法：
#   bash install-macos.sh            # 安装 GitHub 上最新的正式版
#   bash install-macos.sh v1.0.0     # 安装指定版本
#
# 流程：从 GitHub Release 下载 universal dmg → 安装到 /Applications
#      → 移除 com.apple.quarantine 隔离属性 → 启动应用
# 说明：安装包未做 Apple 公证，本脚本的作用就是替用户完成「手动放行」这一步。

set -euo pipefail

REPO="Wayward-GOU/FinkMeter"
APP_NAME="FinkMeter"
APP_BUNDLE="${APP_NAME}.app"
INSTALL_DIR="/Applications"

VERSION="${1:-}"
TAG_NAME=""
MOUNT_POINT=""
TMP_DIR=""

info()  { printf '\033[36m==>\033[0m %s\n' "$*"; }
warn()  { printf '\033[33m!!\033[0m %s\n' "$*"; }
error() { printf '\033[31mxx\033[0m %s\n' "$*" >&2; exit 1; }

cleanup() {
  if [ -n "${MOUNT_POINT}" ]; then
    hdiutil detach "${MOUNT_POINT}" -quiet -force >/dev/null 2>&1 || true
  fi
  if [ -n "${TMP_DIR}" ]; then
    rm -rf "${TMP_DIR}"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

[ "$(uname -s)" = "Darwin" ] || error "该脚本仅支持 macOS。"
for cmd in curl hdiutil xattr sw_vers; do
  command -v "${cmd}" >/dev/null 2>&1 || error "缺少必需命令：${cmd}"
done

MACOS_MAJOR="$(sw_vers -productVersion | cut -d. -f1)"
if [ "${MACOS_MAJOR}" -lt 11 ]; then
  error "需要 macOS 11.0 或更高版本，当前为 $(sw_vers -productVersion)。"
fi

if [ -n "${VERSION}" ]; then
  API_URL="https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"
else
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
fi

json_first_string() {
  # $1: 字段名  $2: JSON 文本，取第一个字符串值
  printf '%s' "$2" |
    grep -o "\"$1\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" |
    head -1 |
    sed 's/^[^:]*:[[:space:]]*"//; s/"$//'
}

# 私有仓库需要 Token 才能读取 Release；公开仓库留空即可
api_get() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL -H 'Accept: application/vnd.github+json' \
      -H "Authorization: Bearer ${GITHUB_TOKEN}" "$1"
  else
    curl -fsSL -H 'Accept: application/vnd.github+json' "$1"
  fi
}

download_dmg() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fL --progress-bar -H "Authorization: Bearer ${GITHUB_TOKEN}" \
      -o "$1" "$2"
  else
    curl -fL --progress-bar -o "$1" "$2"
  fi
}

info "获取 Release 信息..."
RELEASE_JSON="$(api_get "${API_URL}")" \
  || error "无法获取 Release（版本不存在、仍为草稿，私有仓库需先设置 GITHUB_TOKEN，或网络异常）。"

TAG_NAME="$(json_first_string tag_name "${RELEASE_JSON}")"
DMG_URL="$(printf '%s' "${RELEASE_JSON}" \
  | grep -o '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*\.dmg"' \
  | head -1 \
  | sed 's/^[^:]*:[[:space:]]*"//; s/"$//')"

[ -n "${DMG_URL}" ] || error "该版本未提供 dmg 安装包，请到 Releases 页面检查。"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/finkmeter.XXXXXX")"
DMG_PATH="${TMP_DIR}/${APP_NAME}.dmg"

info "下载 ${TAG_NAME} ..."
info "${DMG_URL}"
download_dmg "${DMG_PATH}" "${DMG_URL}" \
  || error "下载失败，请检查网络后重试。"

info "挂载安装镜像..."
ATTACH_OUT="$(hdiutil attach -nobrowse -readonly "${DMG_PATH}")" \
  || error "挂载 dmg 失败（文件可能已损坏，请重新下载）。"
MOUNT_POINT="$(printf '%s\n' "${ATTACH_OUT}" | grep -o '/Volumes/.*' | tail -1)"
[ -n "${MOUNT_POINT}" ] || error "未能定位挂载点。"

SRC_APP="${MOUNT_POINT}/${APP_BUNDLE}"
[ -d "${SRC_APP}" ] || error "镜像中未找到 ${APP_BUNDLE}。"

osascript -e "tell application \"${APP_NAME}\" to quit" >/dev/null 2>&1 || true

DEST_APP="${INSTALL_DIR}/${APP_BUNDLE}"
if [ -d "${DEST_APP}" ]; then
  info "移除已安装的旧版本..."
  rm -rf "${DEST_APP}" 2>/dev/null || sudo rm -rf "${DEST_APP}"
fi

info "安装到 ${INSTALL_DIR} ..."
if ! cp -R "${SRC_APP}" "${DEST_APP}" 2>/dev/null; then
  warn "写入 ${INSTALL_DIR} 需要管理员权限，请输入密码："
  sudo cp -R "${SRC_APP}" "${DEST_APP}"
fi

info "移除 Gatekeeper 隔离属性..."
if ! xattr -dr com.apple.quarantine "${DEST_APP}" 2>/dev/null; then
  sudo xattr -dr com.apple.quarantine "${DEST_APP}" \
    || warn "去除隔离属性失败，首次打开时请右键应用选择「打开」。"
fi

info "完成，正在启动 ${APP_NAME}（${TAG_NAME}）..."
open "${DEST_APP}"

printf '\n'
info "安装路径：${DEST_APP}"
info "如仍被系统拦截，请到「系统设置 → 隐私与安全性」底部点击「仍要打开」。"
