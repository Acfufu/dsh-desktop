#!/usr/bin/env bash
# dev 一键流（spec §9）：cargo tauri dev（beforeDevCommand 拉 vite；sidecar 由 App 自身 ProcessManager 管理）
# R5 修正：不再预启 sidecar——M4 后 App 会 probe socket，外部实例被判定「已在运行」
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export DSH_HOME="${DSH_HOME:-$HOME/.dsh-dev}"
mkdir -p "$DSH_HOME"
cd "$ROOT/src-tauri" && npx tauri dev
