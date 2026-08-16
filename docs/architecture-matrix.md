# 架构矩阵（spec §8 dev/release 差异）

| 维度 | dev（`cargo tauri dev`） | release（.app） |
|---|---|---|
| 前端供给 | vite dev server（port 1420，strictPort） | `frontend/dist`（frontendDist，__DSH_BOOT__ 注入产物） |
| CSP | 严格串 + vite HMR 需 `connect-src ws://localhost:1420`（当前：HMR 受限，手动刷新；release 严格） | `connect-src 'self' ipc: http://ipc.localhost`（无 HMR） |
| 导航白名单 | tauri://localhost + http://ipc.localhost + `http://localhost:1420`（debug 门控，端口精确） | tauri://localhost + http://ipc.localhost |
| sidecar 位置 | `src-tauri/resources/dsh`（DSH_SIDECAR_DIR 可覆盖） | `.app/Contents/Resources/dsh`（resource_dir） |
| sidecar spawn | App 自身 ProcessManager（勿外部预启） | 同左 |
| devtools | 窗口级 `devtools: false`（conf） | 同左 |
| 插件 bundle 供给 | vite `public/plugins/`（dev 静态） | `dist/plugins/`（manifest 派生，39 条，非硬编码） |
| 日志 | `~/Library/Logs/dsh-desktop/sidecar.log`（0600） | 同左 |
| socket | `$DSH_HOME/run/dsh.sock`（0600，目录 0700，lstat/owner 校验） | 同左 |
| e2e | `e2e-smoke.sh dev`（外部预启 sidecar） | `e2e-smoke.sh release`（app 自 spawn，零预启） |
| 签名/公证 | 无 | `scripts/notarize.sh`（需 Apple 凭据） |

## 风险行（spec §8 记录）

- pnpm deploy 对 `file:` vendor 依赖生成指向构建机的绝对/越界符号链接（深度敏感）——`materialize-node-modules.sh` 以「deploy 上下文解析 → 真实内容复制」修复；`rsync -a` 保留 store 内相对链接（深度不变则有效）。
- harness root tsdown 的 workspace 一次构建在 root `lib/types` 缺失时整体失败（陈旧产物被 clean 后不可再生）——sidecar 管线改为：per-package tsdown + esbuild bundle（`--packages=external` + createRequire banner）+ typert generator 直出。
- harness clean 会删除全部构建产物（tsbuildinfo 陈旧 → tsc 跳过 emit）——管线容忍 build 失败，以现有 lib 输出为准（pin 校验仍强制）。
