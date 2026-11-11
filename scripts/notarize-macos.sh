#!/usr/bin/env bash
# macOS Developer ID 签名 + 公证 + 装订（W4 / A-20）
#
# 用法（macOS，仓库根目录）：
#   export MACOS_SIGN_IDENTITY='Developer ID Application: Your Name (TEAMID)'
#   export APPLE_ID='you@example.com'
#   export APPLE_TEAM_ID='TEAMID'
#   export APPLE_APP_PASSWORD='xxxx-xxxx-xxxx-xxxx'   # App 专用密码
#   bash scripts/notarize-macos.sh 'src-tauri/target/universal-apple-darwin/release/bundle/macos/AstroMan.app'
#
# 未配置公证凭据时只做 Developer ID 签名与本地校验，便于离线自测；正式发版必须走完 notarytool。
set -euo pipefail

APP_PATH="${1:-}"
if [[ -z "${APP_PATH}" ]]; then
  echo "用法：bash scripts/notarize-macos.sh <AstroMan.app 路径>" >&2
  exit 2
fi
if [[ ! -d "${APP_PATH}" ]]; then
  echo "应用不存在：${APP_PATH}" >&2
  exit 2
fi

SIGN_IDENTITY="${MACOS_SIGN_IDENTITY:-}"
if [[ -z "${SIGN_IDENTITY}" ]]; then
  echo "未设置 MACOS_SIGN_IDENTITY，当前仅做 ad-hoc 签名" >&2
  codesign --force --deep --sign - "${APP_PATH}"
else
  echo "使用 Developer ID 签名：${SIGN_IDENTITY}"
  codesign --force --deep --options runtime --timestamp \
    --sign "${SIGN_IDENTITY}" "${APP_PATH}"
fi

codesign --verify --deep --strict --verbose=2 "${APP_PATH}"
echo "签名校验通过"

if [[ -z "${APPLE_ID:-}" || -z "${APPLE_TEAM_ID:-}" || -z "${APPLE_APP_PASSWORD:-}" ]]; then
  echo "未配置 APPLE_ID / APPLE_TEAM_ID / APPLE_APP_PASSWORD，跳过公证" >&2
  exit 0
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT
ZIP_PATH="${WORK_DIR}/AstroMan.zip"
ditto -c -k --keepParent "${APP_PATH}" "${ZIP_PATH}"

echo "提交公证（通常需要 1-5 分钟）…"
xcrun notarytool submit "${ZIP_PATH}" \
  --apple-id "${APPLE_ID}" \
  --team-id "${APPLE_TEAM_ID}" \
  --password "${APPLE_APP_PASSWORD}" \
  --wait

xcrun stapler staple "${APP_PATH}"
xcrun stapler validate "${APP_PATH}"
spctl -a -vvv -t execute "${APP_PATH}"

echo "PASS: macOS 签名、公证与装订完成"
