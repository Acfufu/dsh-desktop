#!/usr/bin/env bash
# 同步 fork 拷贝目录与上游（spec §4.3 同步策略：pin 拉取 → diff → 手动合并）
# canary：web-api-client.ts 是 transport 缝漂移哨兵——fork 已删，diff 上游若出现新改动即告警。
set -euo pipefail
REPO="${DSH_REPO:-$HOME/codehub/deepseek-harness}"
PIN="frontend/UPSTREAM_PIN"
COMMIT="$(grep '^COMMIT=' "$PIN" | cut -d= -f2)"
SRC="$REPO/packages/client"
DST="frontend/packages/client"

for p in web modules connection web-react ui-slots ui-primitives ui-attachment schema-form; do
  echo "=== $p ==="
  diff -rq "$SRC/$p/src" "$DST/$p/src" 2>&1 | grep -v '^Only in' || true
done

# canary：上游 web-api-client.ts 有改动 = 协议层可能漂移
if [ -f "$SRC/connection/src/client/web-api-client.ts" ]; then
  echo "CANARY: upstream web-api-client.ts exists (fork deleted it by design)."
  echo "  Protocol-layer drift check: review its diff before bumping UPSTREAM_PIN."
fi
echo "sync check complete (commit $COMMIT)"
