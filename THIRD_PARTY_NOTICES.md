# THIRD_PARTY_NOTICES

再分发组件许可清单（每条含实证来源；无法实证的标注 UNVERIFIED）。

| 组件 | 版本 | 许可 | 实证来源 |
|---|---|---|---|
| deepseek-harness（dsh 全家桶，fork 拷贝 + sidecar 包目录） | 0.1.0-rc.5（repo）/ rc.6（npm） | MIT | `~/codehub/deepseek-harness/package.json` license 字段 + repo `LICENSE`（2026-08-16 实证）；fork 拷贝文件保留上游头 |
| ws | 8.21.3 | MIT | `node_modules/ws/package.json` license 字段 |
| react / react-dom | 18.3.1 | MIT | `frontend/node_modules/react*/package.json` |
| @deepseek-ai/cordis | 4.0.1 | MIT | `host-patch/node_modules/@deepseek-ai/cordis/package.json` |
| @deepseek-ai/dsh-client-*（web/modules/connection/web-react/ui-*/schema-form/runtime/ui-theme） | 0.1.0-rc.6 | MIT | `frontend/node_modules/@deepseek-ai/*/package.json` |
| @deepseek-ai/dsh-host-apiproxy | 0.1.0-rc.6 | MIT | `frontend/node_modules/@deepseek-ai/dsh-host-apiproxy/package.json` |
| @tauri-apps/api + tauri 插件（notification/autostart/opener/single-instance） | 2.11.x | Apache-2.0 OR MIT | `frontend/node_modules/@tauri-apps/api/package.json`；Rust crates LICENSE（cargo registry） |
| node.js（sidecar 运行时二进制） | v24.14.1 | MIT（含附加声明） | `bin/node` 自带 `dist/LICENSE`（node 发行版）；`node --version` 实证 |
| koffi（@koromix/koffi-darwin-arm64） | 3.1.1 | MIT | 包内 LICENSE（实证：包目录存在 LICENSE） |
| sharp（@img/sharp-darwin-arm64 + libvips） | 0.35.3 | Apache-2.0 | 包内 LICENSE |
| node-pty | 1.1.0 | MIT | 包内 LICENSE |
| GPL-3.0（本仓库根 LICENSE） | — | GPL-3.0 | 本仓库 `LICENSE`（项目自身许可，2026-08-16 定） |

> 注：sidecar 包目录闭包内含约 300+ npm 包——完整逐包清单由 `pnpm deploy` 的锁闭闭包构成；上表覆盖直接再分发面（运行时二进制 + fork 源码 + 壳依赖）。完整机器可查清单：`src-tauri/resources/dsh/node_modules/**/package.json` 的 license 字段。deepseek-harness 为 MIT；再分发符合其许可条款。
