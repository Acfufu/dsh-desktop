#!/usr/bin/env bash
# fork client bundles（transport 替换核心）：connection 包必须由 fork 源码构建
# （上游产物是 WebApiClient → fetch tauri://localhost 404 → 桌面永远连不上）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/frontend"
(cd packages/client/connection && COREPACK_ENABLE_STRICT=0 pnpm exec tsdown --config tsdown.client.config.ts > /dev/null 2>&1)
echo "fork connection client bundle: $(ls -la packages/client/connection/lib/client.js | awk '{print $5}') bytes"
