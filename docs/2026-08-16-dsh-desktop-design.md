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
2. **UDS 载体复用 host 已有机制**：patch 插件 = UDS node:http 服务 + `bridge`+`toFetchHandler(apiProxy)`（uplink，均来自公开导出）+ `WebSocketDownlinks`（downlink，vendor 拷贝），协议语义与 web 载体同构（帧格式、信封、状态码语义一致；物理 HTTP 头不承诺逐字节）
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
- **socket 路径选择（Rust 与插件共享同一逻辑）**：候选路径长度 >100 字节时依次回退 `$DSH_HOME/run/dsh.sock` → `os.tmpdir()/dsh-<uid>/dsh.sock` → `/tmp/dsh-<uid>/dsh.sock`（macOS `sockaddr_un.sun_path` 上限 104；长用户名或自定义 `$DSH_HOME` 可超限）。0600 + getpeereid 已防 /tmp 多用户风险
- **socket 残留清理在 carrier 侧**：`listen` 前先 connect 探测（无活服务则 unlink 旧文件再 bind）；Rust 侧重启前清理仅作兜底（node listen 到残留路径文件会 EADDRINUSE，即使无监听者）

desktop.patch.yml 内容（应用顺序在 web-app bundle 之后）：

- `disabled: true`：`webserver`、`web-runtime`、`connection`、`client-hmr`、`modules`（后两者 `inject: ['webServer']`，webserver 禁用后无 provider，不禁则 Loader 结算失败 —— **必禁，非可选项**；随之 `__DSH_BOOT__` 无来源，由 fork 自产，见 4.3）
- `insert`：`uds-carrier` 行，config 含 uds 路径（默认 `$DSH_HOME/run/dsh.sock`；`$DSH_HOME` 不可写时回退 `os.tmpdir()/dsh-<uid>/dsh.sock`）

### 4.2 Rust 侧（tauri crate，哑管道）

- **uplink**：`#[tauri::command] dsh_http(method, path, body) -> { status, headers, body }`——**reqwest 0.12.23+**（`ClientBuilder::unix_socket`）对 UDS 路径发起 HTTP/1.1（URL `http://dsh/api/...`，Host 头自动为 `dsh`，node:http 接受任意 Host）；**不设自身超时**（unary 超时/取消由前端 `AbstractApiClient` 语义决定）；sidecar 重启后重建 client（丢弃连接池死连接）。返回体为原始字节 `Vec<u8>`（二进制保真，前端以 ArrayBuffer 重建 Response body）。同路径共 **3 条独立连接**（1 uplink reqwest + 2 downlink WS），无多路复用需求
- **downlink（前端驱动，无自主重连）**：两路 `tokio-tungstenite`（0.28/0.29，关 TLS）over `tokio::net::UnixStream`；**只在收到前端开流请求时建立**（每请求一新连接）；帧经 `tauri::ipc::Channel<Vec<u8>>` 按序投递（官方推荐 ordered/high-throughput 通道，事件 emit 在异步 listener 下不保证顺序）；**任一流断开 → 不发帧、不重连，直接终止该 channel（`-end` 语义）**，由前端 ConnectionController 按既有代际语义重建——重连节奏唯一归属前端（500ms→10s cap），Rust 不做 WS 层退避
- **进程管理**：spawn sidecar（`dsh web --patch <desktop.patch.yml>`），**cwd 设为 `$DSH_HOME`**（`.env` 的 project 层位置确定；加载序：继承 env > cwd .env > `$DSH_HOME/.env`，GUI 启动无 shell env 时后两层生效，API key 由此进入，不额外处理）；stdout/stderr 捕获（启动失败诊断）；exit → 退避重启（1s → 封顶 30s，连续失败 5 次 → 托盘弹窗 + 停止；**首次启动失败不计入该计数**，走对话框重试/退出）；**spawn 前**清理残留 socket 文件（含首次启动，上次硬杀残留）；AppExit → SIGTERM → 5s grace → SIGKILL（`RunEvent::ExitRequested` → `prevent_exit()` → 异步关闭任务 → `exit(0)`）。二进制经 `bundle.resources` 放入 `Contents/Resources/`，Rust 用 `app.path().resource_dir()` 手动 spawn（优于 externalBin 的 target-triple 改名约束）；发布包需对 sidecar 代码签名（dev 不受影响）。App 崩溃/被 kill 时 sidecar 孤儿化，v1 接受（可选：sidecar 父进程存活探测）
- **下行重连归属**：唯一归属前端 ConnectionController（base 500ms、factor 2、cap 10s）；Rust 退避仅用于 sidecar 进程重启，两层不叠加
- **通知**：前端经 IPC 调 `@tauri-apps/plugin-notification`（fork 内完成，Rust 不解析协议；macOS 通知无需 Info.plist 权限声明，capability 加 `notification:default`）
- **托盘**：Tauri v2 内置 tray API（无需插件）（显示/隐藏窗口、退出）；关闭窗口 = 隐藏到托盘（`exitOnLastWindowClosed: false`）
- **自启**：`tauri-plugin-autostart`（macOS 走 LaunchAgent）
- **WKWebView 基线**：`bundle.macOS.minimumSystemVersion: "12.0"`（锁 ES2022；10.15 自带 Safari 13.1 无 ES2022，React 18+Vite 默认产物会挂）

### 4.3 前端 fork（dsh-desktop-frontend）

- 复制上游 `apps/web` + `packages/client/connection` 浏览器半 + `dsh-client-web` boot 内核（`packages/client/web/src/boot.tsx`），改动面锁在：
  - **同形替换 connection 浏览器插件实现**（27 个 `ui-*` 插件注入 `connection` service —— 不能去掉，只能换载体）：复用 `ConnectionHandle`/`ConnectionController`/`IApiClient` 类型与逻辑，仅 transport 换 Tauri IPC
  - **下行 transport 机制（`openMux`/`openHost` 覆写，前端驱动）**：先创建 `Channel` 并注册 `onmessage`，再 `invoke('dsh_open_stream', { stream, channel })`——invoke 返回即 onOpen 信号（readiness 依赖）；帧按到达序经 `serverRequestSchema` 解析 + **逐帧调用 `onEnvelope` tap**（settings/credentials 安全观察依赖）后入队；channel 终止 → 迭代器正常结束 → 代际失败路径；迭代器 finally → `invoke('dsh_close_stream')` 通知 Rust 关闭对应 WS。帧事件名精确（`dsh:downlink:mux|host` + 终止 `-end`），不用通配符
  - **uplink transport 机制（`doFetch` 覆写）**：前端 AbortSignal 映射为 Rust 侧请求取消（invoke 携带请求 id + 独立取消通道；Rust 不设自身超时，`host.pickDirectory` 无默认超时、`command.execute` 可超 30s 依赖此）；响应字节以 ArrayBuffer 保真重建 Response body（附件图片等二进制走此路）；invoke 传输错误折算为代际失败（经 describe/流失败路径），不得吞成业务错误
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

1. 启动：App → spawn sidecar → **轮询 UDS socket 就绪（connect 探测，ECONNREFUSED/ENOENT 视为未就绪；对未就绪 socket 的 invoke 快速失败不挂起）** → WebView 加载 fork dist → 前端 boot → 前端驱动开流（Channel + invoke）→ 两路 WS 建立 → `host.describe` 握手 → connected
2. 用户输入：前端 → invoke → Rust → UDS `/api/*`（方法透传）→ apiProxy → host 处理
3. host 推送（会话事件/审批/提问）：apiProxy → WebSocketDownlinks → UDS WS 帧 → Rust Channel 投递 → 前端 `onmessage` → ConnectionController → UI
4. 审批/问答应答：前端 → invoke → Rust → UDS `POST /api/respond`
5. 通知：前端收到 agent 完成事件 → `notification` 插件（不经 Rust 解析）
6. 退出：Cmd+Q / 托盘退出 → Rust SIGTERM sidecar → 5s → SIGKILL → 清理 socket → App exit

## 6. 错误处理

| 场景 | 行为 |
|---|---|
| sidecar 启动失败（缺二进制/缺 key 等） | 对话框展示 stderr，提供重试与退出；**不计入崩溃退避计数** |
| sidecar 崩溃 | 退避重启 1s→30s 封顶；连续 5 次失败 → 托盘弹窗 + 停止自动重启 |
| UDS 连接失败 / WS 断线 | 前端 ConnectionController 代际重建（唯一重连所有者）；Rust 侧对未就绪 socket 快速失败 |
| 退出超时 | SIGTERM 后 5s 未退 → SIGKILL |
| socket 残留 | carrier 侧 bind 前 connect 探测 + unlink；Rust spawn 前清理兜底；权限不符（0600 失败）→ 启动失败并报错 |
| 大 body（附件上传 160 MiB 上限、`session.export` 流式下载） | **>~10 MiB 一律走临时文件交接**（invoke 大 payload 实测代价高：150MB 响应约 23s）：上传 = 前端写文件 → Rust 读文件转发；下载 = Rust 落盘 → 前端读；MB 级以下 invoke 直传 |

## 7. 测试

- Rust：uplink/downlink 管道单测（mock 一个 UDS HTTP+WS 服务端；含 Channel 顺序、取消传播、路径回退链超长用例）；进程管理单测（假 sidecar：正常退出/崩溃码/挂起）
- uds-carrier 插件：单测（UDS 请求 → apiProxy 命中、两路 WS 帧、权限拒绝、socket 清理、残留 socket 探测 unlink）
- e2e smoke（`tauri dev`）：起真 sidecar → UDS 连接 → `host.describe` 成功 → WebView 渲染 fork dist → 无 key 时界面就绪；有 `DEEPSEEK_API_KEY` 时可跑一个真实 agent 回合
- 通知：mock notification 插件，验证完成事件触发

## 8. 风险与兜底

| 风险 | 兜底 |
|---|---|
| bun compile 的 `import.meta.url` 资源解析行为不符预期 | 薄自定义入口显式设置资源根 |
| `WebSocketDownlinks` 不在 npm 公开导出面（vendor 拷贝） | vendor 拷贝随上游变更手工同步；锁版本 |
| Tauri 事件 emit 乱序 / 大 payload 慢（下行帧序、附件字节） | 下行用 `tauri::ipc::Channel`（顺序保证）；>~10 MiB 走临时文件交接 |
| UDS 路径超长（`sun_path` 104 字节上限） | 三级回退链（`$DSH_HOME/run` → `os.tmpdir()/dsh-<uid>` → `/tmp/dsh-<uid>`）；单测覆盖长路径 |
| 系统 WebView JS 兼容（10.15 Safari 13.1 无 ES2022） | `minimumSystemVersion: "12.0"` |
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
