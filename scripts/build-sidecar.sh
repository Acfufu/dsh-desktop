#!/usr/bin/env bash
# sidecar 构建（node 运行时 + 包目录布局，spec §4.4 主方案；bun compile 远期，不在本计划）
set -euo pipefail
REPO="${DSH_REPO:-$HOME/codehub/deepseek-harness}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"   # DeepSec L3：此前未定义，$ROOT/tmp-cli-deploy 落到 /tmp-cli-deploy
OUT="$ROOT/src-tauri/resources/dsh"
PIN="$ROOT/host-patch/UPSTREAM_PIN"
COMMIT="$(grep '^COMMIT=' "$PIN" | cut -d= -f2)"

[[ -d "$REPO" ]] || { echo "DSH_REPO not found at $REPO (set DSH_REPO)"; exit 1; }
git -C "$REPO" rev-parse HEAD | grep -q "^$COMMIT" || {
  echo "WARN: $REPO HEAD != $COMMIT; run 'git -C $REPO checkout $COMMIT' to match pin"; exit 1;
}

echo "==> pnpm install + build (web profile closure; 实证：harness root lib/types 为陈旧产物，clean 后 tsc noEmit 不再产出——boot 不需要它，build 失败不阻塞)"
cd "$REPO"
pnpm install 2>&1 | tail -2 || true
pnpm run build > /tmp/dsh-harness-build.log 2>&1 || echo "WARN: harness build failed (see /tmp/dsh-harness-build.log); using existing lib outputs" 

echo "==> assemble $OUT"
rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/lib" "$OUT/config" "$OUT/patch"
NODE_BIN="$(command -v node)"
cp "$NODE_BIN" "$OUT/bin/node"                       # node 二进制（^22.19 || >=24；本机 24.14.1）
cp "$REPO/apps/cli/package.json" "$OUT/package.json" # INSTALL_ANCHOR 命中（lib/../package.json）
cp -r "$REPO/apps/cli/lib/." "$OUT/lib/"             # lib/bin.js + lib/types/
cp -r "$REPO/apps/cli/config/." "$OUT/config/"       # SHIPPED_PRESET_ROOT（lib/../config/agent-presets）
cp "$ROOT/host-patch/desktop.patch.yml" "$OUT/patch/desktop.patch.yml"

echo "==> node_modules 打包（实证流程：deploy + 逃逸物化 + 运行时闭包 + 原生二进制）"
"$ROOT/scripts/materialize-node-modules.sh"

echo "==> verify"
"$OUT/bin/node" -e "console.log('node ok', process.version)"
ls "$OUT"
echo "sidecar assembled (pin $COMMIT). Size: $(du -sh "$OUT" | cut -f1)"
