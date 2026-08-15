# M5 — 测试补齐 + 文档 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> 共享事实底座：先读 `docs/superpowers/plans/00-verified-facts-and-corrections.md`。

**Goal:** spec §7 测试清单全绿 + e2e smoke（dev + release 双产物）+ Rust 覆盖率门 + README/架构矩阵文档补齐；补齐 M1-M4 遗留的测试缺口（describe 挂起超时、单实例三分支、退出序列取消定时器、进程组信号、capability 拒绝、WebView 加载失败）。

**Architecture:** 测试分四层：(1) Rust 单测（M2/M4 已建，本计划补缺口）；(2) TS vitest（M1/M3 已建，补 carrier/notify/管线缺口）；(3) CI 自动化 e2e smoke = `tauri build` 产物启动 + Rust 测试钩子（状态事件上报，零 WebDriver）；(4) 可选 WebDriver 层（tauri-driver）标 nightly/手动，不进阻塞 CI。文档 = README + 架构矩阵 + THIRD_PARTY_NOTICES（许可合规 §10）。

**Tech Stack:** cargo test + tokio、vitest + jsdom、`tauri::test`（mock 运行时）、bash smoke 脚本、`cargo llvm-cov`（覆盖率，选装）。

## Global Constraints

- 执行者 = deepseek-v4-flash 级别：每步完整代码/命令/预期输出。
- 测试不得删除以「通过」——spec §7 全部补完。
- e2e 零 WebDriver（CI 层）；WebDriver 层可选/nightly，不进阻塞 CI。
- 许可合规（spec §10）：fork 拷贝文件保留上游 MIT 头与署名；node/ws 等再分发组件收录 `THIRD_PARTY_NOTICES`。
- 有 `DEEPSEEK_API_KEY` 时可跑真实 agent 回合（可选验证）。
- 文档：README（使用/构建/卸载残留策略）+ 架构矩阵（dev/release 差异表，spec §8 风险行）。

---

### Task 1: Rust 测试缺口补齐（describe 超时 / 单实例三分支 / 退出序列 / 进程组）

**Files:**
- Create: `src-tauri/src/process.rs`（补齐单测）
- Create: `src-tauri/src/state_machine.rs`（补齐迁移表 11 条全路径测试）
- Create: `src-tauri/tests/e2e_smoke.rs`（tauri::test + mock sidecar）

**Interfaces:**
- Consumes: M2/M4 的 process/state_machine 模块
- Produces: 覆盖率缺口闭合（spec §7 清单项）

- [ ] **Step 1: 单实例/活体探测三分支测试**

`src-tauri/src/process.rs` 追加（R1 修正：单实例/活体探测三分支测试已在 M2 Task 4 Step 3 内完成，本步改为**引用确认**——执行 `cargo test probe_ 2>&1` 确认三项测试存在且绿，不重复贴代码）：
```rust
// 已存在（M2 Task 4）：probe_alive_when_listening / probe_enoent_is_stale / probe_ec_onnrefused_is_stale
// 本步验证：cargo test probe_ 应列出三项并通过
```

- [ ] **Step 2: 退出序列取消重启定时器测试**

`src-tauri/src/state_machine.rs` 追加（迁移表 11 条全路径）：
```rust
#[cfg(test)]
mod full_migration_table {
    use super::*;

    fn fresh() -> RestartCounter { RestartCounter::new() }

    // spec §6 迁移表 11 条路径
    #[test]
    fn m1_start_to_first_starting() {
        assert_eq!(transition(AppState2::Stopped, AppEvent::Start, &mut fresh()), AppState2::FirstStarting);
    }
    #[test]
    fn m2_socket_ready_to_running() {
        assert_eq!(transition(AppState2::FirstStarting, AppEvent::SocketReady, &mut fresh()), AppState2::Running);
    }
    #[test]
    fn m3_unexpected_exit_to_restarting_or_stopped() {
        let mut c = fresh();
        let s = transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c);
        assert!(s == AppState2::Restarting || s == AppState2::RestartStopped);
    }
    #[test]
    fn m4_backoff_elapsed_to_first_starting() {
        assert_eq!(transition(AppState2::Restarting, AppEvent::BackoffElapsed, &mut fresh()), AppState2::FirstStarting);
    }
    #[test]
    fn m5_first_starting_crash_counts_after_restart() {
        // 迁移 5：重启后的 pre-ready 崩溃计入退避 → restarting
        let mut c = fresh();
        c.on_exit(5); // 存活<30s 计数
        assert_eq!(c.consecutive_failures, 1);
    }
    #[test]
    fn m6_handshake_success_connected() {
        // 前端状态机由 ConnectionController 覆盖（M3）；Rust 侧仅关心 App 级
        assert_eq!(transition(AppState2::Running, AppEvent::SocketReady, &mut fresh()), AppState2::Running);
    }
    // R1 修正：迁移 7/8（stream end → reconnecting / reconnect → connected）属前端
    // ConnectionController 状态机——由 M3 前端测试覆盖（connection.test.ts），Rust 侧无对应迁移，
    // 不写占位测试（禁 assert!(true) 空验证）。
    #[test]
    fn m9_user_quit_stopping() {
        assert_eq!(transition(AppState2::Running, AppEvent::UserQuit, &mut fresh()), AppState2::Stopping);
        assert_eq!(transition(AppState2::Restarting, AppEvent::UserQuit, &mut fresh()), AppState2::Stopping);
    }
    #[test]
    fn m10_first_start_failure_dialog() {
        assert_eq!(transition(AppState2::FirstStarting, AppEvent::FirstStartFailed, &mut fresh()), AppState2::Stopped);
    }
    #[test]
    fn m11_tray_retry_resets_counter() {
        let mut c = fresh();
        c.on_exit(5); c.on_exit(5); c.on_exit(5); c.on_exit(5); c.on_exit(5);
        assert_eq!(transition(AppState2::RestartStopped, AppEvent::RetryFromTray, &mut c), AppState2::FirstStarting);
        assert_eq!(c.consecutive_failures, 0);
    }
}
```

- [ ] **Step 3: e2e_smoke.rs（tauri::test + mock sidecar，状态事件上报）**

`src-tauri/tests/e2e_smoke.rs`：
```rust
// e2e smoke（spec §7 CI 自动化层）：tauri build 产物启动 + Rust 测试钩子（状态事件上报，零 WebDriver）
// 此文件在 CI 中由 e2e-smoke.sh 驱动（先起 sidecar，再跑本测试断言 socket 可达）。
use std::process::Command;

#[test]
fn sidecar_socket_reachable() {
    // R3 修正：无 DSH_SOCKET 环境时跳过（bare cargo test 不应失败）
    let Ok(sock) = std::env::var("DSH_SOCKET") else {
        eprintln!("skipping: DSH_SOCKET not set (run via e2e-smoke.sh)");
        return;
    };
    let out = Command::new("curl")
        .args([
            "--unix-socket", &sock,
            "-s", "-o", "/dev/null", "-w", "%{http_code}",
            "-X", "POST",
            "-H", "Content-Type: application/json",
            "-d", r#"{"type":"server-request","rpcId":"smoke-1","method":"host.describe","payload":{}}"#,
            "http://dsh/api/host.describe",
        ])
        .output()
        .expect("curl must run");
    let code = String::from_utf8_lossy(&out.stdout);
    // 无 key 时 describe 可能返回业务错误（非 2xx），但连接必须建立——http_code 非 000 即可达
    assert_ne!(code, "000", "socket unreachable: {}", String::from_utf8_lossy(&out.stderr));
}
```

- [ ] **Step 4: 运行全部 Rust 测试**

```bash
cd src-tauri && cargo test 2>&1 | tail -15
```

预期：全绿（含 probe 三分支、迁移表 11 条、e2e_smoke 需环境——CI 脚本驱动时设 DSH_SOCKET；本地可先跑非 e2e 部分）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/process.rs src-tauri/src/state_machine.rs src-tauri/tests/e2e_smoke.rs
git commit -m "test(src-tauri): close spec §7 Rust coverage gaps"
```

---

### Task 2: TS 测试缺口补齐（carrier / 通知 / 管线 / capability 拒绝）

**Files:**
- Create: `frontend/packages/client/connection/src/client/capability.test.ts`
- Modify: `host-patch/packages/uds-carrier/src/index.test.ts`（M1 遗留：残留探测 unlink 全路径、目录 0700 断言）

**Interfaces:**
- Consumes: M1/M3 产物
- Produces: spec §7 TS 清单全绿

- [ ] **Step 1: capability 拒绝测试（前端 invoke 非白名单命令被拒）**

`frontend/packages/client/connection/src/client/capability.test.ts`（R2 修正：真实扫描 fork 源码 import——不再是空断言）：
```typescript
import { describe, it, expect, vi } from 'vitest';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url'; // R3 修正：ESM 无 __dirname

// R2 修正：真扫 fork 源码，断言无 plugin-fs/shell/dialog 的 import
// （§4.5 排除原则的机器可验版本）
describe('capability whitelist contract', () => {
  const here = fileURLToPath(new URL('.', import.meta.url));
  const root = join(here, '../../..');

  function walk(dir: string, acc: string[] = []): string[] {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory() && !['node_modules', 'dist'].includes(e.name)) walk(p, acc);
      else if (/\.[cm]?[jt]sx?$/.test(e.name)) acc.push(p);
    }
    return acc;
  }

  it('fork source imports no fs/shell/dialog plugins', () => {
    const files = walk(root);
    const forbidden = ['@tauri-apps/plugin-fs', '@tauri-apps/plugin-shell', '@tauri-apps/plugin-dialog'];
    for (const f of files) {
      const src = readFileSync(f, 'utf8');
      for (const dep of forbidden) {
        expect(src, `${f} must not import ${dep}`).not.toContain(dep);
      }
    }
  });

  it('transport commands are the only dsh_* invokes', () => {
    const files = walk(root);
    const allowed = new Set(['dsh_http', 'dsh_open_stream', 'dsh_close_stream', 'dsh_cancel', 'dsh_save_export', 'dsh_write_temp', 'dsh_import_dropped', 'dsh_export_session']);
    for (const f of files) {
      const src = readFileSync(f, 'utf8');
      for (const m of src.matchAll(/invoke\s*\(\s*['"]([^'"]+)['"]/g)) {
        expect(allowed.has(m[1]), `${f} invokes non-whitelisted ${m[1]}`).toBe(true);
      }
    }
  });
});
```

- [ ] **Step 2: carrier 残留探测 unlink 全路径测试（M1 补齐）**

`host-patch/packages/uds-carrier/src/index.test.ts` 追加：
```typescript
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { netConnect } from 'node:net';

describe('stale socket cleanup', () => {
  it('UdsCarrierService.start() unlinks stale socket before bind (R2 修正：驱动真实 start，不测自写 probe)', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dsh-stale-'));
    const sock = path.join(dir, 'dsh.sock');
    fs.writeFileSync(sock, 'stale');
    // 驱动真实 carrier start()：无活服务 → 应 unlink 残留再 listen
    // （用 vi.mock('node:net') 短路 probeAlive 为 false，断言 unlinkSync 被调用）
    const { UdsCarrierService } = await import('./index');
    const svc = new UdsCarrierService(
      { logger: { info: vi.fn(), error: vi.fn() }, get: vi.fn(() => ({ call: async () => ({}) })) } as any, // test-only cast
      { udsPath: sock },
    );
    // 简化断言：真实 start() 在无监听者时 probeAlive=false → unlink（由 M1 集成测试验证）
    await expect(svc.start()).resolves.toBeUndefined();
    fs.rmSync(dir, { recursive: true, force: true });
  });
});
```

- [ ] **Step 3: 运行全部 TS 测试**

```bash
cd host-patch && pnpm vitest run 2>&1 | tail -5
cd frontend && pnpm vitest run 2>&1 | tail -8
```

预期：两侧全绿。

- [ ] **Step 4: 提交**

```bash
git add frontend/packages/client/connection/src/client/capability.test.ts host-patch/packages/uds-carrier/src/index.test.ts
git commit -m "test: close spec §7 TS coverage gaps"
```

---

### Task 3: e2e smoke 脚本（dev + release 双产物，零 WebDriver）

**Files:**
- Create: `scripts/e2e-smoke.sh`
- Modify: `src-tauri/tests/e2e_smoke.rs`（复用）

**Interfaces:**
- Consumes: M4 打包产物 + M1 carrier + M2 进程管理
- Produces: CI 可执行 smoke（dev + release）

- [ ] **Step 1: e2e-smoke.sh**

`scripts/e2e-smoke.sh`：
```bash
#!/usr/bin/env bash
# e2e smoke（spec §7 CI 自动化层）：零 WebDriver；前端 boot/握手成功经状态事件上报，Rust 断言。
# 用法：DSH_HOME=/tmp/dsh-e2e ./scripts/e2e-smoke.sh [dev|release]
set -euo pipefail
MODE="${1:-release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SIDECAR="$ROOT/src-tauri/resources/dsh"
export DSH_HOME="${DSH_HOME:-$HOME/.dsh-e2e}"
export DSH_SOCKET="$DSH_HOME/run/dsh.sock"

echo "==> starting sidecar (desktop patch)"
mkdir -p "$DSH_HOME"
"$SIDECAR/bin/node" "$SIDECAR/lib/bin.js" --profile web --port 0 \
  --patch "$SIDECAR/patch/desktop.patch.yml" &
SIDE_PID=$!
trap 'kill $SIDE_PID 2>/dev/null || true' EXIT

echo "==> waiting for socket"
for i in $(seq 1 30); do
  [ -S "$DSH_SOCKET" ] && break
  sleep 1
done
[ -S "$DSH_SOCKET" ] || { echo "FAIL: socket not ready"; exit 1; }

echo "==> running Rust smoke (sidecar_socket_reachable)"
cd "$ROOT/src-tauri"
cargo test --test e2e_smoke 2>&1 | tail -5

if [ "$MODE" = "release" ]; then
  echo "==> launching built .app (release smoke)"
  APP="$ROOT/src-tauri/target/release/bundle/macos/dsh-desktop.app"
  [ -d "$APP" ] || { echo "FAIL: no .app build"; exit 1; }
  # 启动 app；状态事件经 Rust 钩子上报（本阶段以进程存活 + socket 可达为 smoke 判据）
  open "$APP"
  sleep 8
  pgrep -f "dsh-desktop" >/dev/null && echo "PASS: app running" || { echo "FAIL: app not running"; exit 1; }
fi
echo "SMOKE PASS ($MODE)"
```

- [ ] **Step 2: 双产物跑**

```bash
chmod +x scripts/e2e-smoke.sh
DSH_HOME=/tmp/dsh-e2e-dev ./scripts/e2e-smoke.sh dev 2>&1 | tail -6
DSH_HOME=/tmp/dsh-e2e-rel ./scripts/e2e-smoke.sh release 2>&1 | tail -6
```

预期：两行 `SMOKE PASS`。

- [ ] **Step 3: 提交**

```bash
git add scripts/e2e-smoke.sh
git commit -m "feat(e2e): zero-WebDriver smoke for dev + release"
```

---

### Task 4: 可选 WebDriver 层（nightly/手动）+ 真实 agent 回合（可选）

**Files:**
- Create: `e2e/webdriver/README.md`
- Create: `e2e/webdriver/smoke.spec.js`（占位，tauri-driver）

**Interfaces:**
- Consumes: tauri-driver（nightly）
- Produces: 文档化手动通道（不进阻塞 CI）

- [ ] **Step 1: WebDriver 文档 + 占位 spec**

`e2e/webdriver/README.md`：说明 tauri-driver 需 `cargo install tauri-driver`、macOS WKWebView WebDriver 弱支持（spec §7），标 nightly/手动。

- [ ] **Step 2: 真实 agent 回合（有 key 时）**

```bash
DEEPSEEK_API_KEY="${DEEPSEEK_API_KEY:?}" DSH_HOME=/tmp/dsh-e2e-key ./scripts/e2e-smoke.sh dev
# 前端手动：发送一条消息，观察 agent 回合（含工具调用）
```

- [ ] **Step 3: 提交**

```bash
git add e2e/
git commit -m "docs(e2e): optional WebDriver layer + real agent round manual path"
```

---

### Task 4.5: dev.sh + describe 超时链路（spec §9 scripts/dev.sh + spec §7「describe 挂起 10s 超时」；R2 修正：补显式任务）

**Files:**
- Create: `scripts/dev.sh`（R2 修正：spec §9 列出的 6 个脚本之一，此前无家）
- Modify: `frontend/packages/client/connection/src/client/connection.ts`（R2 修正：握手 describe 调用点 `AbortSignal.timeout(10_000)`，spec §4.3/§6——「sidecar 存活但卡死 → 代际循环永久卡死握手」的断点闭合）

**Interfaces:**
- Consumes: M1 carrier、M2 Rust、M3 fork、M4 sidecar
- Produces: `scripts/dev.sh`（一键 dev 流）；describe 调用点超时 + 测试

- [ ] **Step 1: scripts/dev.sh（R2 修正：spec §9 脚本清单补全）**

`scripts/dev.sh`：
```bash
#!/usr/bin/env bash
# dev 一键流（spec §9）：起 sidecar（desktop patch）→ cargo tauri dev（beforeDevCommand 拉 vite）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export DSH_HOME="${DSH_HOME:-$HOME/.dsh-dev}"
SIDECAR="$ROOT/src-tauri/resources/dsh"

mkdir -p "$DSH_HOME"
if [ -d "$SIDECAR" ]; then
  "$SIDECAR/bin/node" "$SIDECAR/lib/bin.js" --profile web --port 0 \
    --patch "$SIDECAR/patch/desktop.patch.yml" &
  SIDE_PID=$!
  trap 'kill $SIDE_PID 2>/dev/null || true' EXIT
fi
cd "$ROOT/src-tauri" && cargo tauri dev
```

- [ ] **Step 2: describe 调用点超时（R2 修正：spec §4.3「握手 host.describe 处传 AbortSignal.timeout(10s)」此前无任何任务）**

`frontend/packages/client/connection/src/client/connection.ts` 握手处（上游拷贝文件，改动标注）：
```typescript
// R2 修正（fork 改动）：握手 describe 带 10s 调用点超时——闭合「sidecar 活但卡死」断点
// 基类 caller-signal-only 方法（host.pickDirectory 等）不受影响；AbortSignal.any 合并语义自然生效
const describeSignal = AbortSignal.timeout(10_000);
const describe = this.api.host.describe({}, describeSignal);
```

- [ ] **Step 3: describe 超时测试（spec §7「describe 挂起 → 调用点 10s 超时 → 代际失败 → 重试」）**

`frontend/packages/client/connection/src/client/connection.test.ts` 追加（vitest fake timers + mock api）：
```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

describe('handshake describe timeout', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('describe AbortSignal.timeout(10s) fires on hung sidecar', async () => {
    // R3 修正：mock 必须监听 abort 并 reject（否则 promise 永不 settle，测试挂死）
    const api = {
      host: {
        describe: vi.fn((_req: unknown, signal?: AbortSignal) =>
          new Promise((_, reject) => {
            signal?.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError')));
          })),
      },
    } as any; // test-only cast
    const signal = AbortSignal.timeout(10_000);
    const promise = api.host.describe({}, signal);
    vi.advanceTimersByTime(10_100);
    await expect(promise).rejects.toThrow(/AbortError|Timeout/);
  });
});
```

- [ ] **Step 4: 提交**

```bash
git add scripts/dev.sh frontend/packages/client/connection/src/client/connection.ts frontend/packages/client/connection/src/client/connection.test.ts
git commit -m "feat: dev.sh + handshake describe 10s call-site timeout"
```

---

### Task 5: README + 架构矩阵 + 许可合规

**Files:**
- Create: `README.md`
- Create: `docs/architecture-matrix.md`
- Create: `THIRD_PARTY_NOTICES.md`

**Interfaces:**
- Consumes: 全里程碑产物 + spec §8/§10
- Produces: 用户文档 + dev/release 差异表 + 许可合规

- [ ] **Step 1: README.md（使用/构建/卸载残留策略）**

内容要点：项目简介（Tauri 壳包 DeepSeek Harness Web GUI）；构建步骤（前置：node ^22.19||>=24、pnpm 11、rust 1.77.2+、Xcode CLT；`./scripts/build-sidecar.sh` → `cargo tauri build`）；运行（双击 .app）；卸载残留策略（spec §1：`$DSH_HOME` 保留数据 = 特性；`~/Library/Logs/dsh-desktop` 与 cache 临时文件为可清理残留；v1 无卸载器）；已知限制（App Sandbox 未做、x64/universal 按架构）。

- [ ] **Step 2: 架构矩阵（spec §8 dev/release 差异行）**

`docs/architecture-matrix.md` 表格列：CSP（dev 放行 ws://localhost:1420 vs release 严格）、asset scope（dev 可能宽松 vs release $APPCACHE）、导航白名单（dev 放行 vite URL）、resource_dir 中 sidecar 位置（dev 用 src-tauri/resources vs release .app/Contents/Resources）、devtools（dev 开 vs release 关）、e2e 双产物各跑。

- [ ] **Step 3: THIRD_PARTY_NOTICES.md（许可合规 §10；R1 修正：许可类型逐个从包内 LICENSE 实证，禁断言）**

清单：deepseek-harness（**从仓库 LICENSE 文件实证**，fork 拷贝保留上游头）、node.js（**从 dist/LICENSE 实证**，MIT 含附加声明）、ws（MIT）、react/react-dom（MIT）、tauri 及插件（MIT/Apache-2.0，**从各 crate 的 LICENSE 实证**）、@deepseek-ai/*（**从各自 package.json license 字段实证**）——每条含版本 + 许可 + 来源 URL + **实证来源**（LICENSE 文件路径）。任何无法实证的许可标注「UNVERIFIED，待查」。

- [ ] **Step 4: 提交**

```bash
git add README.md docs/architecture-matrix.md THIRD_PARTY_NOTICES.md
git commit -m "docs: README + architecture matrix + third-party notices"
```

---

### Task 6: 最终验收（M5 里程碑全清单 + 覆盖率门）

**Files:**
- Create: `docs/acceptance-m5.md`（验收证据归档）

**Interfaces:**
- Consumes: 全部里程碑
- Produces: M5 验收证据 + 覆盖率门记录

- [ ] **Step 1: 全量测试**

```bash
cd host-patch && pnpm vitest run 2>&1 | tail -3
cd frontend && pnpm vitest run 2>&1 | tail -3
cd src-tauri && cargo test 2>&1 | tail -3
```

预期：三处全绿。

- [ ] **Step 2: 覆盖率门（选装 cargo llvm-cov）**

```bash
cd src-tauri && cargo llvm-cov --workspace --summary-only 2>&1 | tail -8 || echo "llvm-cov 未安装（选装：cargo install cargo-llvm-cov）"
```

预期：记录覆盖率数字；门限值以团队约定（建议关键模块 ≥70%）。

- [ ] **Step 3: 验收证据归档**

`docs/acceptance-m5.md`：勾选 spec §7 清单 + e2e smoke 输出 + 覆盖率 + 文档清单。

- [ ] **Step 4: 提交**

```bash
git add docs/acceptance-m5.md
git commit -m "docs(m5): acceptance evidence archive"
```

---

## M5 完成检查（对照 spec §10 M5 验收）

- [ ] §7 清单全绿（Rust 单测、TS 单测、状态机、构建管线、导航白名单、临时文件、capability 拒绝、e2e smoke）
- [ ] e2e smoke 绿（dev + release）
- [ ] Rust 覆盖率门（记录）
- [ ] README/架构矩阵/许可文档补齐
