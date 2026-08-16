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

echo "==> vendor runtime bundle restore (cordis/cosmokit/group/hmr/include/timer: main=lib/index.js 由 root tsdown 产出，root entry 缺口时缺失)"
for v in cordis cosmokit group hmr include timer; do
  MAIN="$OUT/@deepseek-ai/$v/lib/index.js"
  if [ ! -f "$MAIN" ] && [ -f "$OUT/@deepseek-ai/$v/lib/types/index.js" ]; then
    (cd "$REPO/vendor/$v" && COREPACK_ENABLE_STRICT=0 pnpm exec esbuild "lib/types/index.js" --bundle --format=esm --platform=node --target=es2024 --outfile="lib/index.js" --packages=external --tsconfig-raw='{}' --banner:js="import { createRequire as __dshCr } from 'node:module'; const require = __dshCr(import.meta.url);" > /dev/null 2>&1)
    cp "$REPO/vendor/$v/lib/index.js" "$MAIN" && echo "  restored $v"
  fi
done

echo "==> runtime bundle restore（tsconfig paths 别名会把 @deepseek-ai/* 指向源码被内联 → 双实例 symbol 破坏 scope——必须 --tsconfig-raw='{}' 禁别名 + --packages=external）"
for pj in $(find "$REPO/packages" "$REPO/vendor" -maxdepth 3 -name package.json | grep -v node_modules); do
  dir=$(dirname "$pj")
  main=$(node -e "const p=require('$dir/package.json'); console.log(p.main||'')" 2>/dev/null)
  case "$main" in lib/index.js|lib/*.js) ;; *) continue;; esac
  entry="$dir/lib/types/index.js"
  if [ -f "$entry" ] && [ ! -f "$dir/$main" ]; then
    (cd "$dir" && COREPACK_ENABLE_STRICT=0 pnpm exec esbuild "lib/types/index.js" --bundle --format=esm --platform=node --target=es2024 --outfile="lib/index.js" --packages=external --tsconfig-raw='{}' --banner:js="import { createRequire as __dshCr } from 'node:module'; const require = __dshCr(import.meta.url);" > /dev/null 2>&1)
  fi
done

echo "==> typert generated artifacts (root tsdown 缺口时由 generator 直出)"
(cd "$REPO" && cat > tmp-typert-gen.mts << 'TYPEOF'
import { writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { WorkspaceTypertGenerator } from './packages/typert/generator/src/workspace.ts';
const gen = new WorkspaceTypertGenerator(process.cwd());
const pkgs = ['@deepseek-ai/dsh-commands','@deepseek-ai/dsh-goal','@deepseek-ai/dsh-host-plugin-inventory','@deepseek-ai/dsh-message-feedback','@deepseek-ai/dsh-cordis-host-runner'];
for (const a of gen.generate(pkgs, ['host'])) {
  const out = join(process.cwd(), a.packageRoot, 'lib');
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, `typert.${a.face}.js`), a.js);
  writeFileSync(join(out, `typert.${a.face}.d.ts`), a.dts);
  if (a.remote !== undefined) {
    writeFileSync(join(out, 'typert.remote-client.js'), a.remote.js);
    writeFileSync(join(out, 'typert.remote-client.d.ts'), a.remote.dts);
    writeFileSync(join(out, 'typert.remote-client.d.ts.map'), a.remote.dtsMap);
  }
}
TYPEOF
COREPACK_ENABLE_STRICT=0 pnpm exec tsx tmp-typert-gen.mts > /dev/null 2>&1; rm -f tmp-typert-gen.mts)

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
