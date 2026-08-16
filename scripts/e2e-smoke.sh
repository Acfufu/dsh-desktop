#!/usr/bin/env bash
# e2e smoke（spec §7 CI 自动化层）：零 WebDriver；Rust 断言 socket 可达。
# R5 修正：release 模式不得预启 sidecar——App 自身 ProcessManager 会 probe Alive → 弹「已在运行」并退出；
# dev 模式（未接 ProcessManager 的旧产物）才允许外部预启。
set -euo pipefail
MODE="${1:-release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SIDECAR="$ROOT/src-tauri/resources/dsh"
export DSH_HOME="${DSH_HOME:-$HOME/.dsh-e2e}"
export DSH_SOCKET="$DSH_HOME/run/dsh.sock"

if [ "$MODE" = "dev" ]; then
  echo "==> starting sidecar (desktop patch) [dev only]"
  mkdir -p "$DSH_HOME"
  "$SIDECAR/bin/node" "$SIDECAR/lib/bin.js" --profile web \
    --patch "$SIDECAR/patch/desktop.patch.yml" &
  SIDE_PID=$!
  trap 'kill $SIDE_PID 2>/dev/null || true' EXIT

  echo "==> waiting for socket"
  for i in $(seq 1 30); do
    [ -S "$DSH_SOCKET" ] && break
    sleep 1
  done
  [ -S "$DSH_SOCKET" ] || { echo "FAIL: socket not ready"; exit 1; }
fi

echo "==> running Rust smoke (sidecar_socket_reachable)"
cd "$ROOT/src-tauri"
cargo test --test e2e_smoke 2>&1 | tail -5

if [ "$MODE" = "release" ]; then
  echo "==> launching built .app (release smoke; app self-manages sidecar)"
  APP="$ROOT/src-tauri/target/release/bundle/macos/dsh-desktop.app"
  [ -d "$APP" ] || { echo "FAIL: no .app build"; exit 1; }
  open "$APP"
  sleep 8
  for i in $(seq 1 15); do
    [ -S "$DSH_SOCKET" ] && break
    sleep 1
  done
  [ -S "$DSH_SOCKET" ] && echo "PASS: app running + sidecar up" || { echo "FAIL: app or sidecar not running"; exit 1; }
fi
echo "SMOKE PASS ($MODE)"
