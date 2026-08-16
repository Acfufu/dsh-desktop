#!/usr/bin/env bash
# 同步 uds-carrier 的 vendor 文件，防与上游漂移（spec §8：同包双拷贝 hash 哨兵）。
set -euo pipefail
REPO="${DSH_REPO:-$HOME/codehub/deepseek-harness}"
PIN="host-patch/UPSTREAM_PIN"
[[ -f "$PIN" ]] && COMMIT="$(grep '^COMMIT=' "$PIN" | cut -d= -f2)" || { echo "UPSTREAM_PIN missing"; exit 1; }
SRC="$REPO/packages/client/connection/src"
DEST="host-patch/packages/uds-carrier/vendor"

check() {
  local f="$1"
  if ! cmp -s "$SRC/$f" "$DEST/$f"; then
    echo "DRIFT: $f differs from upstream (expected commit $COMMIT)."
    echo "Run: git -C $REPO rev-parse HEAD  # confirm pin"
    echo "Copy: cp \"$SRC/$f\" \"$DEST/$f\""
    exit 1
  fi
}

check http-bridge.ts
check websocket-downlink.ts
echo "vendor files in sync with $COMMIT"
