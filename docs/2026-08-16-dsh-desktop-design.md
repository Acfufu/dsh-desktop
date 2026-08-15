# dsh-desktop 设计 — DeepSeek Harness 的 Tauri 桌面壳

日期：2026-08-16
状态：approved（用户已确认方向）
目标仓库：`~/codehub/dsh-desktop`（独立项目，仓库外）

## 1. 目标与非目标

### 目标

- 一个 macOS 桌面 App（Tauri v2），把 DeepSeek Harness 的 Web GUI 变成原生应用
- 原生体验：Dock 图标、Cmd+Tab、托盘常驻、系统通知、开机自启
- 进程管理：双击即用，host 进程生命周期由壳管理（启动、退出、崩溃退避重启）
- 桌面 patch 下**不暴露 loopback TCP**：host 的 `/api`（含 RCE 级方法）不监听任何 TCP 端口，杜绝浏览器攻击面（DNS rebinding / 跨站简单 POST / confused deputy）

### 非目标（v1 明确不做）

- 不修改 deepseek-harness 仓库本身（全部改动在仓库外，通过 `--patch` 机制注入）
- 不做 Windows/Linux 平台（macOS 先行，架构不排斥后续）
- 不 fork 前端 UI 组件本身（只换 transport 层，UI 全部来自上游）
- 不实现配置管理 UI（API key 走 dsh 自身 `.env` 加载机制）

## 2. 背景事实（来自 deepseek-harness 源码，2026-08-16 现状）

- `dsh web` = `--profile web`，web 组合由 `dsh-web-app` bundle patch 定义（`packages/bundle/web-app/cordis.patch.yml`）
- 前端 = `apps/web`（Vite + React 18），构建产物 `dsh-web-frontend` dist
- RPC 协议为四象限消息模型，与物理通道解耦：
  - uplink：`POST /api/<method>`（unary）+ `POST /api/respond`（回填 server-request 的 rpcId）
  - downlink：两条 WebSocket 流 `/api/events.mux`（MuxFrame）与 `/api/events.host`（HostFrame），每条文本消息 = 一个完整 `ServerRequest` JSON；WS 不接受客户端应用消息
  - 协议不变量全部在 TS 基类 `AbstractApiClient`，平台差异只是 `doFetch` transport 方面（架构注记 2026-07-19-gui-layering-and-rpc-protocol 明确预留了 Electron IPC 载体，本设计用 Rust 实现同一缝）
- `dsh-client-connection` 节点半导出可复用件：
  - `toFetchHandler(apiProxy)`（`http-bridge.ts`）—— 把 apiProxy 包装成 fetch handler
  - `WebSocketDownlinks`（`websocket-downlink.ts`）—— noServer 模式的 WS 下行服务，按 pathname 分发 upgrade
  - 该行 `inject: ['webServer']` —— 节点半绑定在 webserver 上，desktop patch 禁用整行
- `dsh-host-webserver` 只支持 TCP（`host: '127.0.0.1' | '0.0.0.0'`），无 UDS 能力
- `dsh web` 支持 `--patch a.yml --patch b.yml`（可重复，应用在 profile 层之后）与 `--port 0`（OS 分配端口，webserver `port: 0` 语义已确认）
- 信任模型：web 载体有 Host/Origin trust fence（`api-request-trust.ts` / `loopback-hostname.ts`）；UDS 载体用 socket 文件权限 + `getpeereid` 替代
- `process.execPath` spawn 仅存在于 Windows 路径（windows-acl runner、win32-dialog）—— macOS 沙箱用系统 `sandbox-exec`（Seatbelt），code-runtime 是 worker_threads 进程内 —— **macOS 上 bun 编译单二进制方案可行**
- web profile 含 `node:sqlite` 但 `openAt: never`（启动不导入）；`worker_threads` 由 code-runtime 使用

## 3. 架构

```
┌─ dsh-desktop.app ─────────────────────────────────┐
│  WebView (WKWebView)                               │
│    fork 前端 dist @ tauri://localhost（自定义协议） │
│    transport: invoke() ↔ Rust（哑管道）             │
│         ↑ Tauri IPC                                 │
│  Rust (tauri main)                                  │
│    • UDS 哑管道：HTTP uplink 转发 + WS downlink 转发 │
│    • 进程管理：spawn/监控/退避重启 sidecar           │
│    • 托盘 / 自启（通知由前端直接调插件）              │
│         ↑ UDS socket（$DSH_HOME 或 runtime dir，0600）│
│  sidecar：bun 编译的 dsh（web profile + desktop patch）│
│    • api-gateway / api-remotes / 会话 / agent 全家桶 │
│    • uds-carrier 插件：UDS node:http 服务             │
└─────────────────────────────────────────────────────┘
```

### 核心决策

1. **协议解析全留前端，Rust 是哑管道**。四象限协议、帧校验、重连逻辑全部在 fork 的 TS 里；Rust 只做 socket 搬运（HTTP 请求/响应、WS 帧原样转发）。Rust 不解析任何业务帧
2. **UDS 载体复用 host 已有机制**：patch 插件 = UDS node:http 服务 + `toFetchHandler(apiProxy)` 接线（uplink）+ `WebSocketDownlinks` 接线（downlink），协议与 web 载体逐字节同构
3. **desktop patch 禁用 TCP 面**：`--patch desktop.patch.yml` 禁用 `webserver` / `web-runtime` / `connection` 等 TCP 载体行，插入 `uds-carrier`。无 loopback TCP 监听
4. **前端 fork 面锁在 transport 层**：fork `apps/web` + client 端 transport，只换物理载体，UI/对象层/会话层全部来自上游；通知在 fork 里作为一等模块
5. **sidecar 不需要 dist 资源**（无 webserver、无前端托管），资源只有 `config/agent-presets` + desktop patch + uds-carrier 插件产物 —— 绕开 bun compile 最大的资源嵌入问题

## 4. 组件

### 4.1 uds-carrier 插件（host 侧，TS）

独立小包（本地目录插件），通过 desktop.patch.yml 挂载：

- `node:http` server `listen(<uds-path>)`，socket `chmod 600`（若宿主平台不支持权限则拒绝启动）
- uplink 路由：`POST /api/*` → `bridge(toFetchHandler(apiProxy))` 语义与 connection 行一致（复用 `http-bridge.ts` 导出）
- downlink：upgrade 按 pathname 分发 `/api/events.mux`、`/api/events.host` → `WebSocketDownlinks`（复用 `websocket-downlink.ts` 导出）
- 信任：连接时 `getpeereid` 校验 peer uid == 本进程 uid；无 HTTP Host/Origin fence 概念
- 大 body：沿用 connection 行同款 `maxRequestBodyBytes` 约束（默认 160MB）
- 生命周期：随 plugin 挂载/卸载；退出时关闭所有连接（参考 webserver 的 `closeAllConnections` 语义）并清理 socket 文件

desktop.patch.yml 内容（应用顺序在 web-app bundle 之后）：

- `disabled: true`：`webserver`、`web-runtime`、`connection`、`client-hmr`（无浏览器场景）、`modules`（视 fork 主入口需要而定，实现时验证）
- `insert`：`uds-carrier` 行，config 含 uds 路径（默认 `$DSH_HOME/run/dsh.sock`；`$DSH_HOME` 不可写时回退 `os.tmpdir()/dsh-<uid>/dsh.sock`）

### 4.2 Rust 侧（tauri crate，哑管道）

- **uplink**：`#[tauri::command] dsh_http(method, path, body) -> { status, headers, body }`——tokio UDS 连接 + HTTP 客户端（`hyper-util` unix connector 或 `reqwest` unix-socket feature，实现时二选一）；请求携带最小合法 HTTP/1.1 头（Host 设为 `dsh`，其余按需）
- **downlink**：两路 `tungstenite` over `tokio::net::UnixStream`（/api/events.mux、/api/events.host），每帧文本消息 → `tauri::Emitter::emit('dsh:downlink:mux' | 'dsh:downlink:host', raw_json)`；断线退避重连（1s → 2s → 4s → 封顶 30s）；两流独立
- **进程管理**：spawn sidecar（`dsh web --patch <desktop.patch.yml>` + 继承 env，API key 依赖 dsh 自身的 `.env` 加载，不额外处理）；stdout/stderr 捕获（启动失败诊断）；exit → 退避重启（1s → 封顶 30s，连续失败 5 次 → 托盘弹窗 + 停止）；AppExit → SIGTERM → 5s grace → SIGKILL；重启时清理残留 socket 文件
- **通知**：Rust 只转发——前端经 IPC 调 `@tauri-apps/plugin-notification`（fork 内完成，Rust 不解析协议）
- **托盘**：tauri tray（显示/隐藏窗口、退出）；关闭窗口 = 隐藏到托盘（配置 `exitOnLastWindowClosed: false`）
- **自启**：`tauri-plugin-autostart`

### 4.3 前端 fork（dsh-desktop-frontend）

- 复制上游 `apps/web` + `packages/client/connection` 的浏览器半，改动面锁在：
  - 新 transport 实现 `IApiClient`（两流语义不变）：uplink → `invoke('dsh_http')`；downlink → `listen('dsh:downlink:*')` 双流 + 现有 ConnectionController 退避重连逻辑尽量复用
  - `main.ts` 接线：去掉 connection 浏览器插件，挂 tauri-transport；`window.__DSH_BOOT__`/modules roster 视需要裁剪（实现时验证）
  - 通知：订阅 agent 完成/提问事件（复用 UI 现有事件订阅，不加协议解析）→ `@tauri-apps/plugin-notification`
- 构建：vite build → 产物作为 Tauri `frontendDist`，WebView 从 `tauri://localhost` 加载
- 同步策略：fork 面锁在 transport 层，上游版本锁 pin；定期合并上游，冲突面预期小

### 4.4 sidecar 构建（bun 编译）

- 输入：deepseek-harness checkout（路径可用环境变量 `DSH_REPO` 配置，默认 `~/codehub/deepseek-harness` 或同级）
- 流程：`pnpm install && pnpm run build` → `bun build --compile` 打 `apps/cli` 的 lib 入口（`lib/bin.js`）
- 资源布局：`config/agent-presets/`、`desktop.patch.yml`、uds-carrier 插件产物 → 二进制旁同层目录（`.app/Contents/Resources/`），按 `import.meta.url` 相对解析（实现时验证 bun 行为；兜底：薄自定义入口，显式设置 preset root）
- 产物：`dsh-desktop` 可执行文件

## 5. 数据流

1. 启动：App → spawn sidecar → 轮询 UDS socket 就绪（sidecar 打印 URL 行或就绪标记）→ Rust 建立两路 WS → WebView 加载 fork dist → 前端 `host.describe` 握手 → connected
2. 用户输入：前端 → invoke → Rust → UDS `POST /api/<method>` → apiProxy → host 处理
3. host 推送（会话事件/审批/提问）：apiProxy → WebSocketDownlinks → UDS WS 帧 → Rust emit → 前端 `listen` → ConnectionController → UI
4. 审批/问答应答：前端 → invoke → Rust → UDS `POST /api/respond`
5. 通知：前端收到 agent 完成事件 → `notification` 插件（不经 Rust 解析）
6. 退出：Cmd+Q / 托盘退出 → Rust SIGTERM sidecar → 5s → SIGKILL → 清理 socket → App exit

## 6. 错误处理

| 场景 | 行为 |
|---|---|
| sidecar 启动失败（缺二进制/缺 key 等） | 对话框展示 stderr，提供重试与退出 |
| sidecar 崩溃 | 退避重启 1s→30s 封顶；连续 5 次失败 → 托盘弹窗 + 停止自动重启 |
| UDS 连接失败 / WS 断线 | 前端现有 ConnectionController 退避重连（协议层无感知） |
| 退出超时 | SIGTERM 后 5s 未退 → SIGKILL |
| socket 残留 | 重启前删除；权限不符（0600 失败）→ 启动失败并报错 |
| 大 body（>IPC 舒适区，如附件） | 实现时验证 invoke 上限；必要时大文件走临时文件路径交接（前端写文件 → Rust 读文件转发），约束仍是 160MB |

## 7. 测试

- Rust：uplink/downlink 管道单测（mock 一个 UDS HTTP+WS 服务端）；进程管理单测（假 sidecar：正常退出/崩溃码/挂起）
- uds-carrier 插件：单测（UDS 请求 → apiProxy 命中、两路 WS 帧、权限拒绝、socket 清理）
- e2e smoke（`tauri dev`）：起真 sidecar → UDS 连接 → `host.describe` 成功 → WebView 渲染 fork dist → 无 key 时界面就绪；有 `DEEPSEEK_API_KEY` 时可跑一个真实 agent 回合
- 通知：mock notification 插件，验证完成事件触发

## 8. 风险与兜底

| 风险 | 兜底 |
|---|---|
| bun compile 的 `import.meta.url` 资源解析行为不符预期 | 薄自定义入口显式设置资源根 |
| `toFetchHandler` / `WebSocketDownlinks` 导出面随上游变化 | 锁版本；变化时改 patch 插件适配点 |
| Tauri IPC 大 payload（附件/工具结果） | 临时文件交接路径 |
| code-runtime worker_threads / `node:sqlite` 在 bun 下的兼容性 | 实测；不过则 sidecar 回退为「node 二进制 + 包目录」布局（进程管理不变） |
| 上游 preview 快速迭代 | fork 锁 transport 层；定期合并 |
| 前端模块 roster（`__DSH_BOOT__`）与 fork 主入口的裁剪 | 实现时验证，必要时 fork main.ts 自行组装 |

## 9. 仓库结构（dsh-desktop）

```
dsh-desktop/
  docs/                        # 本 spec + 后续文档
  src-tauri/                   # Rust: tauri main, UDS 管道, 进程管理, 托盘, 自启
  frontend/                    # fork 的前端（apps/web + connection 浏览器半）
  host-patch/                  # uds-carrier 插件 + desktop.patch.yml
  scripts/                     # sidecar 构建（bun compile）、资源布局、打包
  e2e/                         # smoke 测试
```

## 10. 里程碑

1. M1：uds-carrier 插件 + desktop patch，`dsh web --patch` 手动验证 UDS 上 `host.describe` 可达
2. M2：Rust 哑管道 + 进程管理，CLI 下验证 uplink/downlink 转发
3. M3：前端 fork + transport 替换，`tauri dev` 跑通完整聊天
4. M4：托盘 / 自启 / 通知 / 生命周期打磨，打包 `.app`
5. M5：测试补齐 + 文档
