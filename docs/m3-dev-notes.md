# M3 Dev-mode Acceptance Evidence

日期：2026-08-16

## 验收 ① dev 栈启动

- `beforeDevCommand: pnpm --dir frontend dev`（vite, port 1420, strictPort）
- sidecar：`DSH_HOME=/tmp/dsh-m3-test` + desktop.patch.yml（UDS 0600，M1 产物）
- `npx tauri dev`：编译通过，窗口渲染 `http://localhost:1420/`（WebView HTML content 确认）

## 验收 ② __DSH_BOOT__ 全条目 load

- vite 服务 HTML 含 `__DSH_BOOT__`（39 entries, rev=4ef0c8a60d7d，派生自运行时组合）
- 39 个 `public/plugins/<id>/client.js` 静态供给；抽查 `dsh-client-runtime`、`dsh-client-ui-conversation` bundle → HTTP 200
- 条目数 = 派生数（非硬编码）：`derive-composed-entries.mjs` 从 `$DSH_HOME/profiles/node_modules` 闭包扫描 `dsh.client` + `lib/client.js` 存在性

## 验收 ③/⑤ 可视状态（GUI 手动步骤，本环境无图像输入未目验）

- WebView 内双流 open → describe → connected 为手动观察项；Rust 侧 `dsh_open_stream`/`dsh_http` 均有单测（M2）
- 有 key agent 回合为手动观察项（DEEPSEEK_API_KEY 未配置）

## 验收 ⑥ 构建管线测试

`pnpm vitest run scripts/` 全绿：buildManifest schema / rev 稳定性 / dist CSP(connect-src ipc:) / boot script 注入 / plugins 完整（10 tests）

## 偏差记录

- composed-entries 输入 = profiles/node_modules 闭包（39 行）；最终组合（desktop patch 禁用 connection/client-hmr/modules 后 ≈33 行）应由 M4 sidecar 运行时清单派生——当前为开发期近似，M4 管线替换。
- CSP `script-src 'self'`（无 nonce）：实证 `injectBootManifest` 产物为内联 script 且不带 nonce，nonce 化会锁死 boot（DeepSec L3 二选一，选移除并在管线测试断言）。
- `devtools: false` 为窗口级选项（非 security），已按 schema 修正。
- vite outDir 相对 root 解析 → `../../dist`（frontend/dist，对齐 tauri frontendDist）。
