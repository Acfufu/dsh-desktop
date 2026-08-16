#!/usr/bin/env bash
# node_modules 闭合（实证流程，2026-08-16）：
# 1) pnpm deploy apps/cli 封闭闭包 → rsync -a（保留 store 内相对 symlink）
# 2) 物化逃逸 symlink（file: vendor/workspace 包 → 复制真实内容，排除 node_modules）
# 3) 运行时组合闭包物化：boot 一次 desktop profile 让 launcher 维护平铺回退（权威闭包），
#    把缺失包复制到顶层（修复 pnpm deploy 对 workspace:^ 依赖裁剪导致的不完整闭包）
# 4) 原生二进制可选依赖（sharp/koffi）从构建机 harness root node_modules 补入
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="${DSH_REPO:-$HOME/codehub/deepseek-harness}"
OUT="$ROOT/src-tauri/resources/dsh/node_modules"
DEPLOY="$ROOT/tmp-cli-deploy"
CLOSURE_HOME="$ROOT/tmp-closure-home"

rm -rf "$DEPLOY" "$OUT" "$CLOSURE_HOME"

echo "==> pnpm deploy (apps/cli closure)"
(cd "$REPO" && pnpm --filter '@deepseek-ai/dsh' deploy --legacy "$DEPLOY" 2>&1 | tail -2)
mkdir -p "$OUT"
rsync -a "$DEPLOY/node_modules/" "$OUT/"

echo "==> materialize escaping symlinks (vendor/workspace file: links)"
cd "$DEPLOY/node_modules"
while IFS= read -r l; do
  rel="${l#./}"
  r=$(readlink -f "$l" 2>/dev/null)
  case "$r" in "$DEPLOY/node_modules"/*) continue;; esac
  if [ -d "$r" ] && [ -f "$r/package.json" ]; then
    rm -f "$OUT/$rel" && mkdir -p "$OUT/$rel" && rsync -a --exclude node_modules --exclude .git --exclude tests "$r/" "$OUT/$rel/"
  fi
done < <(find . -type l)

echo "==> runtime closure materialization (authoritative: launcher flat fallback)"
mkdir -p "$CLOSURE_HOME"
DSH_HOME="$CLOSURE_HOME" node "$REPO/apps/cli/lib/bin.js" --profile web --patch "$ROOT/host-patch/desktop.patch.yml" > /dev/null 2>&1 &
BOOT_PID=$!
sleep 10
kill "$BOOT_PID" 2>/dev/null || true
C="$CLOSURE_HOME/profiles/node_modules"

# scoped（修复 @ 前缀重复：basename 已含 @，不得再加）
for sc in "$C"/@*/; do
  [ -d "$sc" ] || continue
  scname="$(basename "$sc")"
  for d in "$sc"*/; do
    [ -d "$d" ] || continue
    rel="$scname/$(basename "$d")"
    if [ -d "$OUT/$rel" ] && [ -f "$OUT/$rel/package.json" ]; then continue; fi
    rp=$(readlink -f "$d" 2>/dev/null)
    if [ -n "$rp" ] && [ -f "$rp/package.json" ]; then
      mkdir -p "$OUT/$(dirname "$rel")"
      rsync -a --exclude node_modules --exclude .git --exclude tests "$rp/" "$OUT/$rel/"
    fi
  done
done
# flat
for d in "$C"/*/; do
  name="$(basename "$d")"
  case "$name" in @*) continue;; esac
  [ -d "$d" ] || continue
  if [ -d "$OUT/$name" ] && [ -f "$OUT/$name/package.json" ]; then continue; fi
  rp=$(readlink -f "$d" 2>/dev/null)
  if [ -n "$rp" ] && [ -f "$rp/package.json" ]; then
    rsync -a --exclude node_modules --exclude .git --exclude tests "$rp/" "$OUT/$name/"
  fi
done

echo "==> native optional deps (sharp/koffi)"
for p in @img/sharp-darwin-arm64 @img/sharp-libvips-darwin-arm64 @koromix/koffi-darwin-arm64; do
  mkdir -p "$OUT/$(dirname "$p")"
  rm -rf "$OUT/$p"
  if [ -d "$REPO/node_modules/$p" ]; then
    rsync -a "$REPO/node_modules/$p/" "$OUT/$p/"
  else
    echo "WARN: native dep missing in build env: $p"
  fi
done

echo "==> uds-carrier（幂等）"
rm -rf "$OUT/@dsh-desktop"
mkdir -p "$OUT/@dsh-desktop"
cp -r "$ROOT/host-patch/packages/uds-carrier" "$OUT/@dsh-desktop/uds-carrier"
rm -rf "$OUT/@dsh-desktop/uds-carrier/src" "$OUT/@dsh-desktop/uds-carrier/vendor" "$OUT/@dsh-desktop/uds-carrier/tsconfig"*.json

rm -rf "$DEPLOY" "$CLOSURE_HOME"
echo "node_modules closure: $(du -sh "$OUT" | cut -f1)"
