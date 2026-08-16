# dsh-desktop

DeepSeek Harness 的 macOS 桌面壳（Tauri v2）。把 `dsh web` 的 Web GUI 变成原生应用：Dock 图标、托盘常驻、系统通知、开机自启；host 进程生命周期由壳管理（启动、崩溃退避重启）。

**架构一句话**：前端 fork（transport 层替换为 Tauri invoke）↔ Rust 哑管道（UDS socket 搬运）↔ sidecar（node + dsh 包目录布局，`--patch desktop.patch.yml` 禁用全部 TCP 载体，仅 UDS 监听，0600 socket / 0700 目录）。

## 构建

前置：node `^22.19 || >=24`、pnpm 11、rust 1.77.2+（本机 1.97.1）、Xcode CLT；`DSH_REPO`（默认 `~/codehub/deepseek-harness`，pin `47f943859bef60e4160492346772ded9b24f765a`，见 `host-patch/UPSTREAM_PIN`）。

```bash
# 1. sidecar 打包（node + 包目录布局，实证流程：deploy → 逃逸物化 → 运行时闭包 → 原生二进制 → typert 产物）
./scripts/build-sidecar.sh
# 2. 前端构建（fork + transport + manifest 注入）
pnpm --dir frontend build
node --input-type=module -e "..." # 由 generate-manifest 完成 __DSH_BOOT__ 注入（见 frontend/scripts）
# 3. .app 打包
cd src-tauri && npx tauri build --bundles app
# 产物：target/release/bundle/macos/dsh-desktop.app
```

开发模式：`./scripts/dev.sh`（vite + `cargo tauri dev`；sidecar 由 App 自身 ProcessManager 管理，勿外部预启——会被判定「已在运行」）。

## 运行

双击 `.app`。首次启动创建 `$DSH_HOME`（默认 `~/.dsh`，0700）并在 `$DSH_HOME/profiles/web/` 物化 profile（含 `@dsh-desktop/uds-carrier` 链接）；sidecar 由壳 spawn（独立进程组），socket = `$DSH_HOME/run/dsh.sock`（0600）。

## 卸载残留策略（spec §1）

- `$DSH_HOME` 保留用户数据 = 特性（有意设计）
- `~/Library/Logs/dsh-desktop`（sidecar 日志 0600，1 MiB × 3 轮转）与 app cache 临时文件（`temp-uploads`，启动按 24h 年龄清扫）为可清理残留
- v1 无卸载器

## 已知限制

- 未做 App Sandbox（enclave 外）；x64/universal 按架构定案（本机 arm64）
- 公证（notarize）需 Apple Developer 凭据：`scripts/notarize.sh`（环境变量 APPLE_ID/TEAM_ID/APPLE_APP_PASSWORD）
- Typert 远端端点（`/api/commands/execute` 等）在桌面组合下为 404（connection 禁用；carrier 已具备 interceptor 双层分发机制，M3+ 可注册恢复）
- WebDriver 层为 nightly/手动（`e2e/webdriver/`）；阻塞 CI 用 `scripts/e2e-smoke.sh`（零 WebDriver）

## 测试

```bash
cd src-tauri && cargo test            # Rust：transport/进程管理/状态机/日志/临时文件（45+）
cd frontend && pnpm vitest run        # TS：transport/通知/管线/CSP/capability 契约（16）
cd host-patch && pnpm vitest run      # uds-carrier（11）
./scripts/e2e-smoke.sh dev|release    # 零 WebDriver smoke
```

## 结构

- `host-patch/` — desktop patch（禁用 TCP 载体行 + 插入 UDS 载体）与 uds-carrier 插件（vendor 拷贝上游 connection 传输件，`sync-carrier.sh` 哨兵防漂移）
- `src-tauri/` — Rust 壳：`http_command`（dsh_http UDS uplink）、`streams`（downlink 三命令）、`process`/`process_manager`/`state_machine`（spawn/退避/单实例/退出序列）、`navigation`（白名单）、`tempfiles`/`logging`/`dialogs`、`tray`
- `frontend/` — fork 8 上游包 + `tauri-api-client` transport + manifest 管线（`sync-frontend.sh` canary）
- `scripts/` — build-sidecar / materialize-node-modules / sync-carrier / sync-frontend / verify-m1 / e2e-smoke / dev / notarize
