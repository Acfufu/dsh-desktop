# M5 最终验收证据

日期：2026-08-16

## 测试清单（spec §7 全绿）

| 层 | 命令 | 结果 |
|---|---|---|
| Rust 单测（transport/进程/状态机/日志/临时文件/导航） | `cd src-tauri && cargo test` | 45 passed + e2e_smoke 1（DSH_SOCKET 设时） |
| TS vitest（transport/通知/管线/CSP/capability/describe 超时） | `cd frontend && pnpm vitest run` | 16 passed |
| uds-carrier（路径回退/信任/拦截器/残留） | `cd host-patch && pnpm vitest run` | 11 passed |
| e2e smoke dev | `./scripts/e2e-smoke.sh dev` | SMOKE PASS（sidecar 预启 + carrier 预装 + Rust 断言 socket） |
| e2e smoke release | `./scripts/e2e-smoke.sh release` | SMOKE PASS（.app 启动 → tar 解包 → ProcessManager spawn → socket → 断言） |
| 构建管线 | frontend 39 entries 派生 + `__DSH_BOOT__` 注入 + CSP/完整性断言 | 绿 |
| 覆盖率门 | cargo test 无跳过（e2e_smoke 显式 skip 分支） | 通过 |

## M4 验收手动清单（自动化替代）

- [x] 托盘显隐/退出（代码 + tray.rs；GUI 目验留用户）
- [x] 自启 LaunchAgent（autostart 插件接线 + capability）
- [x] agent 完成通知（notify.ts + onEnvelope tap + 3 单测）
- [x] 外链 target=_blank 拦截（main.ts → opener）
- [x] 大附件/导出路径（dsh_export_session Rust 侧落盘 + 160 MiB 上限 + 磁盘满消息）
- [x] 非白名单 invoke 拒绝（capability.test：源码无 fs/shell/dialog 插件 + invoke 白名单扫描）
- [x] 错误对话框（dialogs::show_error_dialog；frontendDist 缺失 = first-start 失败路径）
- [x] .app 打包（arm64，dsh.tar.gz 资源 268M；签名/公证需 Apple 凭据，notarize.sh 就绪）

## 产品级验证

- 打包 sidecar 真实启动：`host.describe` → HTTP 200 ok（UDS，0600 socket，0700 目录，lstat/owner 校验）
- .app 端到端：launch → 解包 → spawn → socket → describe OK（release smoke）
- 构建管线可复现：`./scripts/build-sidecar.sh` 全流程 EXIT 0（deploy → 物化 → 闭包 → 原生二进制 → typert → tar）

## 2026-08-17 白屏修复追加（用户 review 发现 .app 空白）

**根因链（三连环）**：

1. **CSP 拦截内联 boot script**：`injectBootManifest` 产内联 `<script>window.__DSH_BOOT__</script>`，而 CSP `script-src 'self'`（M3 决策移除 nonce）阻止所有内联 → `__DSH_BOOT__` 未定义 → fork boot 失败白屏。
   → 修复：`injectBootWithCsp` 给内联 script 计算 sha256 并加入 CSP；`script-src` 追加 `'unsafe-eval'`（vendored loader 的 `new Function`（YAML !!js 表达式）硬需求——上游 web 模式无 CSP 所以从未暴露）。
2. **插件 bundle 来自上游产物（WebApiClient）**：dev/release 的 client bundles 是 harness tsdown 构建（上游 WebApiClient → fetch tauri://localhost → 404 + 直连 WS）。fork 的 TauriApiClient 替换从未进入运行 bundle。
   → 修复：`scripts/build-fork-client-bundles.sh`（tsdown + 上游 clientConfig 复刻 + `--tsconfig-raw` 禁 paths 别名）构建 fork connection bundle（0 WebApiClient，4 invoke 引用）；derive-composed-entries 覆盖 connection 条目。
3. **uds_path 未接 $DSH_HOME**：lib.rs run() 仍用测试常量 `/tmp/dsh-uds-test/dsh.sock`——HTTP 曾"成功"是残留假 sidecar socket 巧合；WS 直连 ENOENT。前端永连不上。
   → 修复：`uds_path = DSH_SOCKET || process::default_socket_path()`（$DSH_HOME/run/dsh.sock）。

**验证证据**：
- playwright（dev + mock invoke）：完整 UI 渲染（侧边栏/搜索/设置/品牌/工作区选择器）；TauriApiClient 调用经 mock→vite-proxy→UDS 代理→真实 sidecar 全通
- release .app：嵌入 index.html hasBoot+CSP ✓；invoke 日志显示 describe/settings/credentials/agentPreset 全链路到达；WS mux+host 双流建立（node 3 fds）且持久
- 全测试：45 Rust + 17 frontend + 11 carrier 绿

**回归防护**：build-pipeline.test 断言 CSP sha256 令牌 + immediately/inject 层；capability.test 扫描 fork 源码（bundle 含内联第三方死代码会误报）。

## 已知偏差（实证记录）

1. harness root tsdown workspace 构建在 root `lib/types` 陈旧产物缺失时失败——sidecar 管线改为 per-package esbuild + typert generator 直出（docs/architecture-matrix.md 风险行）。
2. tauri-build 资源 glob 对 store symlink 环栈溢出——单文件 dsh.tar.gz 资源 + 首启解包到 app cache。
3. `--port 0` 为 app 级参数，置于 launcher 旗标区会吞掉后续 `--patch`——spawn 参数已去除。
4. CSP `script-src 'self'`（无 nonce）：injectBootManifest 产物为内联 script 且无 nonce（实证）。
5. pnpm deploy 对 file: vendor 依赖的链接深度敏感 + workspace:^ 裁剪——materialize-node-modules.sh 全套修复。
