#!/usr/bin/env bash
# 公证（spec §4.4）：hardened runtime + 公证；需要 Apple Developer 账号环境变量
set -euo pipefail
APP="${1:?usage: notarize.sh path/to/dsh-desktop.app}"
BUNDLE_ID="com.dsh-desktop.app"
# xcrun notarytool 需要 --apple-id/--team-id/--password（环境变量传入，勿落盘）
xcrun notarytool submit "$APP" --wait \
  --apple-id "${APPLE_ID:?}" --team-id "${TEAM_ID:?}" --password "${APPLE_APP_PASSWORD:?}" \
  2>&1 | tail -5
xcrun stapler staple "$APP"
echo "notarized: $APP"
