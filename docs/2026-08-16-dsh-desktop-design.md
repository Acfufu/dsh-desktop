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
  - uplink：`POST /api/<method>`（unary）+ `POST /api/respond`（回填 server-request 的 rpcId）+ `GET/HEAD /api/session.export`（流式 ZIP 下载，session-log-download 行挂载）
  - downlink：两条 WebSocket 流 `/api/events.mux`（MuxFrame）与 `/api/events.host`（HostFrame），每条文本消息 = 一个完整 `ServerRequest` JSON；WS 不接受客户端应用消息
  - 协议不变量全部在 TS 基类 `AbstractApiClient`，平台差异是 transport 方面：`doFetch`（uplink）+ `openMux`/`openHost`（下行流读取，WebApiClient 覆写为 WS 编解码）（架构注记 2026-07-19-gui-layering-and-rpc-protocol 明确预留了 Electron IPC 载体，本设计用 Rust 实现同一缝）
- `dsh-client-connection` 节点半的可复用件（注意导出面）：
  - `toFetchHandler(apiProxy)` 定义在 **`@deepseek-ai/dsh-host-apiproxy`**（`fetch/handler.ts`，根导出），connection 是消费方；`http-bridge.ts` 导出的是 `bridge`（node:http↔fetch 桥）+ `DEFAULT_MAX_REQUEST_BODY_BYTES`（160 MiB）
  - `WebSocketDownlinks`（`websocket-downlink.ts`，构造 `(api: ApiProxy)`，不依赖 webServer，可独立使用）**不在公开导出面**：仅 `./src/*` 子路径可达，且 npm `files` 不含 `src/` —— 外部需 vendor 拷贝或 git checkout 依赖
  - connection 行插件级 `inject: ['webServer']`，web-app patch 行级覆盖为 `inject: [webRuntime]`；节点半经此链在 webserver 上注册 `/api` 路由与两路 upgrade。该行还承担 `PRIVILEGED_METHODS` 环回钉死（settings/credentials/host.openPath 等）；desktop 禁用整行后由 getpeereid（同 uid）取代
- `dsh-host-webserver` 只支持 TCP（`host: '127.0.0.1' | '0.0.0.0'`），无 UDS 能力
- `dsh web` 支持 `--patch a.yml --patch b.yml`（可重复）。patch 应用顺序：bundle 层 → profile 自身 `cordis.patch.yml` → `$DSH_HOME/cordis.patch.yml`（home 层）→ `--patch` overlays → composeProfile 追加的 agent-presets shipped-root（`config.roots` 整键覆盖）→ telemetry switch。**desktop patch 不是最终层**，不得依赖覆盖 `agent-presets.roots`。`--port 0` = OS 分配端口（webserver `port: 0` 语义已确认）
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
2. **UDS 载体复用 host 已有机制**：patch 插件 = UDS node:http 服务 + `bridge`+`toFetchHandler(apiProxy)`（uplink，均来自公开导出）+ `WebSocketDownlinks`（downlink，vendor 拷贝），协议与 web 载体逐字节同构
3. **desktop patch 禁用 TCP 面**：`--patch desktop.patch.yml` 禁用 `webserver` / `web-runtime` / `connection` 等 TCP 载体行，插入 `uds-carrier`。无 loopback TCP 监听
4. **前端 fork 面锁在 transport 层**：fork `apps/web` + client 端 transport，只换物理载体，UI/对象层/会话层全部来自上游；通知在 fork 里作为一等模块
5. **sidecar 不需要 dist 资源**（无 webserver、无前端托管），资源只有 `config/agent-presets` + desktop patch + uds-carrier 插件产物 —— 绕开 bun compile 最大的资源嵌入问题

## 4. 组件

### 4.1 uds-carrier 插件（host 侧，TS）

独立小包（本地目录插件），通过 desktop.patch.yml 挂载：

- `node:http` server `listen(<uds-path>)`，socket `chmod 600`（若宿主平台不支持权限则拒绝启动）
- uplink 路由：`/api/*` 方法透传（POST unary/respond + GET/HEAD session.export）→ `bridge(req, res, toFetchHandler(apiProxy), maxBody)`（`toFetchHandler` 来自 `@deepseek-ai/dsh-host-apiproxy` 根导出；`bridge` 来自 connection `http-bridge.ts`）
- downlink：upgrade 按 pathname 分发 `/api/events.mux`、`/api/events.host` → `WebSocketDownlinks`（vendor 拷贝自 `websocket-downlink.ts`，构造 `(apiProxy)`，不依赖 webServer）
- 信任：连接时 `getpeereid` 校验 peer uid == 本进程 uid；无 HTTP Host/Origin fence 概念
- 大 body：沿用 `DEFAULT_MAX_REQUEST_BODY_BYTES` 约束（160 MiB）
- 生命周期：随 plugin 挂载/卸载；退出时关闭所有连接（参考 webserver 的 `closeAllConnections` 语义）并清理 socket 文件

desktop.patch.yml 内容（应用顺序在 web-app bundle 之后）：

- `disabled: true`：`webserver`、`web-runtime`、`connection`、`client-hmr`、`modules`（后两者 `inject: ['webServer']`，webserver 禁用后无 provider，不禁则 Loader 结算失败 —— **必禁，非可选项**；随之 `__DSH_BOOT__` 无来源，由 fork 自产，见 4.3）
- `insert`：`uds-carrier` 行，config 含 uds 路径（默认 `$DSH_HOME/run/dsh.sock`；`$DSH_HOME` 不可写时回退 `os.tmpdir()/dsh-<uid>/dsh.sock`）

### 4.2 Rust 侧（tauri crate，哑管道）

- **uplink**：`#[tauri::command] dsh_http(method, path, body) -> { status, headers, body }`——tokio UDS 连接 + HTTP 客户端（`hyper-util` unix connector 或 `reqwest` unix-socket feature，实现时二选一）；请求携带最小合法 HTTP/1.1 头（Host 设为 `dsh`，其余按需）
- **downlink**：两路 `tungstenite` over `tokio::net::UnixStream`（/api/events.mux、/api/events.host），每帧文本消息 → `tauri::Emitter::emit('dsh:downlink:mux' | 'dsh:downlink:host', raw_json)`；断线退避重连（1s → 2s → 4s → 封顶 30s）；两流独立
- **进程管理**：spawn sidecar（`dsh web --patch <desktop.patch.yml>`），**cwd 设为 `$DSH_HOME`**（`.env` 的 project 层位置确定；加载序：继承 env > cwd .env > `$DSH_HOME/.env`，GUI 启动无 shell env 时后两层生效，API key 由此进入，不额外处理）；stdout/stderr 捕获（启动失败诊断）；exit → 退避重启（1s → 封顶 30s，连续失败 5 次 → 托盘弹窗 + 停止）；AppExit → SIGTERM → 5s grace → SIGKILL；重启时清理残留 socket 文件
- **downlink 重连**：1s→2s→4s→封顶 30s 是 Rust↔UDS 层退避；前端 ConnectionController 另有独立退避层（base 500ms、factor 2、cap 10s），两层叠加——sidecar 重启期间前端按自身退避持续重试握手
- **通知**：Rust 只转发——前端经 IPC 调 `@tauri-apps/plugin-notification`（fork 内完成，Rust 不解析协议）
- **托盘**：tauri tray（显示/隐藏窗口、退出）；关闭窗口 = 隐藏到托盘（配置 `exitOnLastWindowClosed: false`）
- **自启**：`tauri-plugin-autostart`

### 4.3 前端 fork（dsh-desktop-frontend）

- 复制上游 `apps/web` + `packages/client/connection` 浏览器半 + `dsh-client-web` boot 内核（`packages/client/web/src/boot.tsx`），改动面锁在：
  - **同形替换 connection 浏览器插件实现**（27 个 `ui-*` 插件注入 `connection` service —— 不能去掉，只能换载体）：复用 `ConnectionHandle`/`ConnectionController`/`IApiClient` 类型与逻辑，仅 transport 换 Tauri IPC（uplink → `invoke('dsh_http')`；downlink → `listen('dsh:downlink:*')` 双流）
  - **`__DSH_BOOT__` 自产**：禁用 modules 行后无 manifest 来源，而 shell boot（`boot.tsx`）硬性要求其存在 —— fork 自行注入（可复用 `injectBootManifest` 纯函数）；`apps/web/src/main.ts` 仅 8 行壳，不是改造面
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
2. 用户输入：前端 → invoke → Rust → UDS `/api/*`（方法透传）→ apiProxy → host 处理
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
| 大 body（>IPC 舒适区：附件上传 160 MiB 上限、`session.export` 流式下载） | 实现时验证 invoke 上限；必要时大文件走临时文件路径交接（上传：前端写文件 → Rust 读文件转发；下载：Rust 落盘 → 前端读），约束仍是 160 MiB |

## 7. 测试

- Rust：uplink/downlink 管道单测（mock 一个 UDS HTTP+WS 服务端）；进程管理单测（假 sidecar：正常退出/崩溃码/挂起）
- uds-carrier 插件：单测（UDS 请求 → apiProxy 命中、两路 WS 帧、权限拒绝、socket 清理）
- e2e smoke（`tauri dev`）：起真 sidecar → UDS 连接 → `host.describe` 成功 → WebView 渲染 fork dist → 无 key 时界面就绪；有 `DEEPSEEK_API_KEY` 时可跑一个真实 agent 回合
- 通知：mock notification 插件，验证完成事件触发

## 8. 风险与兜底

| 风险 | 兜底 |
|---|---|
| bun compile 的 `import.meta.url` 资源解析行为不符预期 | 薄自定义入口显式设置资源根 |
| `WebSocketDownlinks` 不在 npm 公开导出面（vendor 拷贝） | vendor 拷贝随上游变更手工同步；锁版本 |
| Tauri IPC 大 payload（附件/工具结果） | 临时文件交接路径 |
| code-runtime worker_threads / `node:sqlite` 在 bun 下的兼容性 | 实测；不过则 sidecar 回退为「node 二进制 + 包目录」布局（进程管理不变） |
| 上游 preview 快速迭代 | fork 锁 transport 层；定期合并 |
| `__DSH_BOOT__` 自产与 boot 内核（`boot.tsx`）的对接面 | 复用 `injectBootManifest`；必要时 fork 自组装入口 |

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
