# dsh-desktop 设计 — DeepSeek Harness 的 Tauri 桌面壳

日期：2026-08-16
状态：approved（用户已确认方向）
目标仓库：`~/codehub/dsh-desktop`（独立项目，仓库外）

## 1. 目标与非目标

### 目标

- 一个 macOS 桌面 App（Tauri v2），把 DeepSeek Harness 的 Web GUI 变成原生应用
- 原生体验：Dock 图标、Cmd+Tab、托盘常驻、系统通知、开机自启
- 进程管理：双击即用，host 进程生命周期由壳管理（启动、退出、崩溃退避重启）
- 桌面 patch 下**不暴露 loopback TCP**：host 的 `/api`（含 RCE 级方法）不监听任何 TCP 端口，消除浏览器与网络可达的攻击面（DNS rebinding / 跨站简单 POST / confused deputy 均依赖浏览器可到达的 HTTP 端点，UDS 对浏览器不可达）；剩余信任边界为同 uid 本地进程（单用户桌面应用的固有信任模型，与终端运行 dsh 的暴露面一致），见 §3「剩余威胁模型」

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
- ConnectionController（前端）代际语义：任一流结束 → 整代失败 → 退避重建（base 500ms、factor 2、cap 10s）；握手 = mux+host 双流 open + `host.describe` 成功；`streamOpenTimeoutMs`（默认 3s）只与双流 open 竞速，**不覆盖 describe**；`stop()`/代际失败 abort 只中止两路流，**不取消在途 unary**

## 3. 架构

```
┌─ dsh-desktop.app ─────────────────────────────────┐
│  WebView (WKWebView)                               │
│    fork 前端 dist @ tauri://localhost（自定义协议） │
│    transport: invoke() ↔ Rust（哑管道）             │
│         ↑ Tauri IPC (capability 白名单)             │
│  Rust (tauri main)                                  │
│    • UDS 哑管道：HTTP uplink 转发 + WS downlink 转发 │
│    • 进程管理：spawn/监控/退避重启 sidecar（进程组）  │
│    • 托盘 / 自启（通知由前端直接调插件）              │
│         ↑ UDS socket（0600，目录 0700）             │
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

### 剩余威胁模型（单用户桌面固有信任）

UDS 端点对浏览器不可达——web 页面无法连接 unix socket（fetch/XHR/WebSocket/EventSource 皆 TCP/QUIC），且无任何 TCP 监听端口，因此 DNS rebinding、跨站简单 POST、confused deputy 三类浏览器攻击与全部网络攻击在桌面载体下不存在。剩余可达者是与本机同 uid 的本地进程：可连接 UDS 调用全部 `/api` 方法（含 settings/credentials 读写与 session.prompt 的 RCE 级能力）、可经 `ps eww` 读取 sidecar 进程环境中的 `DEEPSEEK_API_KEY`、可替换 socket 文件对前端实施中间人。这是单用户桌面应用的固有信任模型——同 uid 进程本就等同用户本人（可直接读 .env、可终止进程），与 dsh 在终端中运行的暴露面一致，desktop 未新增暴露。其他 uid 的本地进程被 socket 文件 0600 + 目录 0700 + 每连接 getpeereid 校验拒之门外。App 内 WebView 的 XSS 面与 web 载体同源 XSS 等价（均可驱动 agent），desktop 通过 capability 最小集（§4.5，仅 transport 命令 + 通知 + 自启）与 asset protocol 严格 scope（§4.6）将新增本地面（文件读、通知、窗口）压至最低。

## 4. 组件

### 4.1 uds-carrier 插件（host 侧，TS）

独立小包（本地目录插件），通过 desktop.patch.yml 挂载：

- `node:http` server `listen(<uds-path>)`，socket `chmod 600`（若宿主平台不支持权限则拒绝启动）
- uplink 路由：`/api/*` 方法透传（POST unary/respond + GET/HEAD session.export）→ `bridge(req, res, toFetchHandler(apiProxy), maxBody)`（`toFetchHandler` 来自 `@deepseek-ai/dsh-host-apiproxy` 根导出；`bridge` 来自 connection `http-bridge.ts`）
- downlink：upgrade 按 pathname **精确匹配**分发 `/api/events.mux`、`/api/events.host` → `WebSocketDownlinks`（vendor 拷贝自 `websocket-downlink.ts`，构造 `(apiProxy)`，不依赖 webServer）
- 信任：**每个接受的连接（含 WS upgrade 连接）在 `connection` 事件统一 `getpeereid` 校验**（peer uid == 本进程 uid）通过后才进入协议处理；无 HTTP Host/Origin fence 概念
- 大 body：沿用 `DEFAULT_MAX_REQUEST_BODY_BYTES` 约束（160 MiB）
- 生命周期：随 plugin 挂载/卸载；退出时关闭所有连接（参考 webserver 的 `closeAllConnections` 语义）并清理 socket 文件
- **socket 路径选择（Rust 与插件共享同一逻辑）**：候选路径长度 >100 字节时依次回退 `$DSH_HOME/run/dsh.sock` → `os.tmpdir()/dsh-<uid>/dsh.sock` → `/tmp/dsh-<uid>/dsh.sock`（macOS `sockaddr_un.sun_path` 上限 104；长用户名或自定义 `$DSH_HOME` 可超限）。**目录一律以 0700 创建且属当前 uid**（防其他 uid 替换 socket 文件实施 bind 劫持——0600+getpeereid 只挡连接，不挡文件替换）；0600 socket + 每连接 getpeereid 已防 /tmp 多用户连接风险
- **socket 残留清理在 carrier 侧**：`listen` 前先 connect 探测（无活服务则 unlink 旧文件再 bind）；Rust 侧重启前清理仅作兜底（node listen 到残留路径文件会 EADDRINUSE，即使无监听者）

desktop.patch.yml 内容（应用顺序在 web-app bundle 之后）：

- `disabled: true`：`webserver`、`web-runtime`、`connection`、`client-hmr`、`modules`（后两者 `inject: ['webServer']`，webserver 禁用后无 provider，不禁则 Loader 结算失败 —— **必禁，非可选项**；随之 `__DSH_BOOT__` 无来源，由 fork 自产，见 4.3）
- `insert`：`uds-carrier` 行，config 含 uds 路径（默认 `$DSH_HOME/run/dsh.sock`；`$DSH_HOME` 不可写时回退 `os.tmpdir()/dsh-<uid>/dsh.sock`）

### 4.2 Rust 侧（tauri crate，哑管道）

- **uplink**：`#[tauri::command] dsh_http(method, path, body) -> { status, headers, body }`——**reqwest 0.12.23+**（`ClientBuilder::unix_socket`）对 UDS 路径发起 HTTP/1.1（URL `http://dsh/api/...`，Host 头自动为 `dsh`，node:http 接受任意 Host）；**不设自身超时**（unary 超时由前端统一施加，见 4.3）；**为 POST 固定 `Content-Type: application/json`**（`dsh_http` 无 headers 参数，且上游 415 媒体类型 fence 在 UDS 载体上仍生效，作为纵深防御）；sidecar 重启后重建 client（丢弃连接池死连接）。返回体为原始字节 `Vec<u8>`（二进制保真，前端以 ArrayBuffer 重建 Response body）。同路径共 **3 条独立连接**（1 uplink reqwest + 2 downlink WS），无多路复用需求
- **uplink 输入校验（纵深防御，非信任边界）**：method ∈ {POST, GET, HEAD}；path 以 `/api/` 开头且不含控制字符/空白（防 `//` 导致的 URL 语义漂移）；未来 carrier 若新增 /api 外路由不会被通用代理意外暴露（前端本身即 /api 全量权限持有者，此校验只防前端 transport 代码缺陷）
- **downlink（前端驱动，无自主重连）**：两路 `tokio-tungstenite`（0.28/0.29，关 TLS）over `tokio::net::UnixStream`；**只在收到前端开流请求时建立**（每请求一新连接，按请求 id 跟踪）；帧经 `tauri::ipc::Channel<Vec<u8>>` 按序投递（官方推荐 ordered/high-throughput 通道，事件 emit 在异步 listener 下不保证顺序）；**任一流断开 → 不发帧、不重连，直接终止该 channel（`-end` 语义）**，由前端 ConnectionController 按既有代际语义重建——重连节奏唯一归属前端（500ms→10s cap），Rust 不做 WS 层退避；`dsh_close_stream` 对未知/未建立 id **幂等 no-op**（open_stream 未完成即代际失败时）
- **进程管理**：spawn sidecar（`dsh web --patch <desktop.patch.yml>`），**cwd 设为 `$DSH_HOME`**（`.env` 的 project 层位置确定；加载序：继承 env > cwd .env > `$DSH_HOME/.env`，GUI 启动无 shell env 时后两层生效，API key 由此进入，不额外处理）；**spawn 时 `setpgid` 建独立进程组**（SIGTERM/SIGKILL 作用于进程组——agent 回合中的 bash/shell 子进程随组收尾，防退出后孤儿进程继续运行/改工作区）；stdout/stderr 捕获（启动失败诊断，日志策略见下）；App 运行期间 sidecar 的任何非 App 驱动的退出（**含 exit 0**）一律按意外退出 → 退避重启（exit code 仅作诊断字段）；退避规则见 §6
- **单实例与活体探测**：App 加 `tauri-plugin-single-instance`；Rust spawn 前 connect 探测——**活体服务存在（connect 成功）则弹「dsh-desktop 已在运行」并退出**，仅 ENOENT/ECONNREFUSED 才 unlink 残留再 spawn（防双 sidecar 共享 `$DSH_HOME` 的 SQLite 双写与活体 socket 误删）
- **退出序列**（`RunEvent::ExitRequested` → `prevent_exit()` → 异步关闭任务）：① 取消挂起的重启定时器/退避 sleep（**期间禁止任何 spawn**）→ ② sidecar 存活则 SIGTERM（进程组，sidecar 优雅 teardown 树杀为第一层）→ ③ 5s grace → ④ SIGKILL（进程组，兜底）→ ⑤ unlink socket + 清临时文件 → ⑥ `exit(0)`。SIGKILL 后 socket 清理由 Rust 执行（parent 可靠）；carrier 自身 teardown 的 unlink 为 best-effort（重复 unlink 幂等）
- **导航白名单**：`WebviewWindowBuilder` 的 `on_navigation` 仅放行 `tauri://localhost` / `http://ipc.localhost`；外链统一经 opener 插件交系统浏览器；fork 侧拦截 `target=_blank`（模型输出 markdown 链接点击不得导航主窗口）
- **日志**：sidecar stdout/stderr → `~/Library/Logs/dsh-desktop/sidecar.log`（1MB × 3 轮转，`from_utf8_lossy` 解码，spawn 时显式设 `LC_ALL=<locale>.UTF-8` 防乱码）；诊断对话框展示 tail 20 行；**key 不落盘**（脱敏：日志中不记录 env 值）；App 自身 `RUST_LOG=info` 落 `dsh-desktop.log` + panic hook 写文件
- **通知**：前端经 IPC 调 `@tauri-apps/plugin-notification`（fork 内完成，Rust 不解析协议；macOS 通知无需 Info.plist 权限声明，capability 加 `notification:default`）
- **托盘**：Tauri v2 内置 tray API（无需插件）（显示/隐藏窗口、退出）；关闭窗口 = 隐藏到托盘（`exitOnLastWindowClosed: false`）
- **自启**：`tauri-plugin-autostart`（macOS 走 LaunchAgent）
- **WKWebView 基线**：`bundle.macOS.minimumSystemVersion: "12.0"`（锁 ES2022；10.15 自带 Safari 13.1 无 ES2022，React 18+Vite 默认产物会挂）；`app.security.devtools: false` 显式关闭（release）
- **App Sandbox**：v1 不做（sandboxed App spawn 的 sidecar 继承沙箱，而 sidecar 跑 bun/bash/fs 全家桶，沙箱内基本不可用或需海量 entitlement）；采用 hardened runtime + 公证 + sidecar 代码签名（bun 需 `allow-unsigned-executable-memory`/`allow-jit` entitlement 的公证注意项）；App Sandbox 记为已知限制/后续工作

### 4.3 前端 fork（dsh-desktop-frontend）

- 复制上游 `apps/web` + `packages/client/connection` 浏览器半 + `dsh-client-web` boot 内核（`packages/client/web/src/boot.tsx`），改动面锁在：
  - **同形替换 connection 浏览器插件实现**（27 个 `ui-*` 插件注入 `connection` service —— 不能去掉，只能换载体）：复用 `ConnectionHandle`/`ConnectionController`/`IApiClient` 类型与逻辑，仅 transport 换 Tauri IPC
  - **下行 transport 机制（`openMux`/`openHost` 覆写，前端驱动）**：先创建 `Channel` 并注册 `onmessage`，再 `invoke('dsh_open_stream', { stream, channel })`——invoke 返回即 onOpen 信号（readiness 依赖）；帧按到达序经 `serverRequestSchema` 解析 + **逐帧调用 `onEnvelope` tap**（settings/credentials 安全观察依赖）后入队；channel 终止 → 迭代器正常结束 → 代际失败路径；迭代器 finally → `invoke('dsh_close_stream')` 通知 Rust 关闭对应 WS（open_stream 未完成即失败时该调用幂等 no-op）。**挂起的 open_stream invoke 绑定代际 AbortSignal**（generation 取消即中止，Rust 侧 open_stream 任务同时观察 sidecar 重启即终止）。帧事件名精确（`dsh:downlink:mux|host` + 终止 `-end`），不用通配符
  - **uplink transport 机制（`doFetch` 覆写）**：前端 AbortSignal 映射为 Rust 侧请求取消（invoke 携带请求 id + 独立取消通道；Rust 不设自身超时，`host.pickDirectory` 无默认超时、`command.execute` 可超 30s 依赖此）；**前端统一施加 unary 超时（含握手 `host.describe`，如 `AbortSignal.timeout(10s)`）**——`streamOpenTimeout` 不覆盖 describe，sidecar 存活但卡死时无此超时则代际循环永久卡死握手；响应字节以 ArrayBuffer 保真重建 Response body（附件图片等二进制走此路）；**传输错误 vs 业务错误分类**：invoke reject（连接拒绝/IO/超时，带 `kind` 字段）→ 可重试传输错误；HTTP status + body → 业务错误
  - **在途 unary 语义**：代际重建**不取消**在途 unary（上游 `stop()`/abort 只中止两路流）——以传输错误 reject，**不吞、不自动重放**，UI 提供重试；代际重建由流终止/握手失败驱动，独立于在途 unary 结果
  - **`__DSH_BOOT__` 自产**：禁用 modules 行后无 manifest 来源，而 shell boot（`boot.tsx`）硬性要求其存在 —— fork 自行注入（可复用 `injectBootManifest` 纯函数）；`apps/web/src/main.ts` 仅 8 行壳，不是改造面
  - **CSP**：fork `index.html` 加 meta CSP：`default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost; font-src 'self' data:`（`blob:` 供附件图片 ArrayBuffer 重建的 blob URL；`ipc:` 为 Tauri v2 IPC 端点）
  - 通知：订阅 agent 完成/提问事件（复用 UI 现有事件订阅，不加协议解析）→ `@tauri-apps/plugin-notification`
- 构建：vite build → 产物作为 Tauri `frontendDist`，WebView 从 `tauri://localhost` 加载
- 同步策略：fork 面锁在 transport 层，上游版本锁 pin；定期合并上游，冲突面预期小

### 4.4 sidecar 构建（bun 编译）

- 输入：deepseek-harness checkout（路径可用环境变量 `DSH_REPO` 配置，默认 `~/codehub/deepseek-harness` 或同级）
- 流程：`pnpm install && pnpm run build` → `bun build --compile` 打 `apps/cli` 的 lib 入口（`lib/bin.js`）
- 资源布局：`config/agent-presets/`、`desktop.patch.yml`、uds-carrier 插件产物 → 二进制旁同层目录（`.app/Contents/Resources/`），按 `import.meta.url` 相对解析（实现时验证 bun 行为；兜底：薄自定义入口，显式设置 preset root）
- 产物：`dsh-desktop` 可执行文件

### 4.5 IPC 面与 capability 最小集

Tauri v2 的 IPC 面 = capability 白名单（XSS 经 invoke 只能调到白名单内命令）。capability 文件按主窗口 label 绑定，最小集：

- `core:default`（窗口/事件/托盘）
- 自定义 transport 命令：`dsh_http`、`dsh_open_stream`、`dsh_close_stream`、`dsh_cancel`、临时文件命令（见 4.6）
- `notification:default`（通知）
- `autostart` 三权限（enable/disable/isEnabled）

**排除原则：不引入 fs / shell / dialog / http / opener 插件，不暴露任何非 transport 命令。** 注意：该白名单约束的是**桌面新增本地面**（文件读、通知、窗口）——agent 驱动能力经 `dsh_http` 本就全量开放（与 web 载体同源 XSS 等价），非白名单所能压缩。通知/自启权限仅本地影响（打扰级），属可接受残余，不设额外机制。

### 4.6 临时文件纪律（>~10 MiB 大 body 交接）

- 目录：app 专属临时子目录（0700，属当前 uid；用 App cache 目录而非 `os.tmpdir()` 裸路径）
- 文件：0600；**文件名由 Rust 随机生成**（绝不采用 Content-Disposition 或任何用户/前端输入——`session.export` 的 zip 内容与文件名由 host 侧 sessionId 生成，前端/用户输入不进路径，此面已排除）
- 读取通道：**asset protocol**（`convertFileSrc`），scope 在 tauri.conf.json `app.security.assetProtocol.scope` **严格限定该临时目录**（绝不能含 `$HOME` 或全盘——scope 设错则 XSS → 任意文件读，这是 desktop 相对 web 的新增本地面）
- 上传通道：<10 MiB 走 invoke 直传；大文件选一种（fs 插件 scope 限定临时目录，或 WKWebView drop 事件给 Rust 原生路径），Rust 对前端传回路径统一 canonicalize 后校验仍在临时目录内
- 清理归属：**下载临时文件归 Rust**（读毕/失败/退出时删除 + 启动时按年龄清扫孤儿）；**上传临时文件归前端**（finally 删除）；App 退出序列（§4.2 ⑤）兜底清扫

## 5. 数据流

1. 启动：App（single-instance 检查 + 活体探测）→ spawn sidecar → **Rust 不 gate 窗口**（WebView 立即加载，socket 未就绪由前端代际循环天然处理——首连即走 regen 路径；Rust 对未就绪 socket 的 invoke 快速失败不挂起）→ 前端 boot → 前端驱动开流（Channel + invoke）→ 两路 WS 建立 → `host.describe` 握手（带前端 unary 超时）→ connected
2. 用户输入：前端 → invoke → Rust → UDS `/api/*`（方法透传）→ apiProxy → host 处理
3. host 推送（会话事件/审批/提问）：apiProxy → WebSocketDownlinks → UDS WS 帧 → Rust Channel 投递 → 前端 `onmessage` → ConnectionController → UI
4. 审批/问答应答：前端 → invoke → Rust → UDS `POST /api/respond`
5. 通知：前端收到 agent 完成事件 → `notification` 插件（不经 Rust 解析）
6. 退出：Cmd+Q / 托盘退出 → Rust 退出序列（取消重启 → SIGTERM 组 → 5s → SIGKILL 组 → 清理 socket + 临时文件）→ App exit

## 6. 错误处理与生命周期

### 状态机

状态两层，各归其主：

**App 级（Rust 进程管理器）：**
- `stopped` — 初始/终态
- `first-starting` — 首次 spawn（**首次 socket-ready 之前**）；失败 → 对话框[重试/退出]，**不计数**
- `running` — 曾达 socket-ready，sidecar 存活
- `restarting` — 意外退出 → 退避（规则见下）
- `restart-stopped` — 连续 5 次 → 托盘弹窗 + 停止；[重试] → 重置计数 → `first-starting`
- `stopping` — 退出序列（§4.2：取消重启 → SIGTERM(组) → 5s → SIGKILL(组) → 清理 → exit）

**连接级（前端 ConnectionController，上游语义不变）：**
- `connecting` — boot 后首握手前
- `connected` — 握手成功
- `reconnecting` — 代际失败 → 退避 500ms→10s

**组合矩阵（UI 呈现）：**

| App 状态 × 连接状态 | UI |
|---|---|
| `first-starting` × `connecting` | 启动中 / 对话框 |
| `running` × `connected` | 正常 |
| `running` × `reconnecting` | 重连指示（>30s 持续 → banner + 诊断入口，防静默降级） |
| `restarting` × `reconnecting` | 重连指示 |
| `restart-stopped` × `reconnecting` | 托盘弹窗 + banner（手动重试） |
| `stopping` / `stopped` | 退出 |

**迁移（触发 → 动作 → 所有者）：**
1. App 启动 → single-instance + 活体探测 → spawn → `first-starting`（Rust）
2. socket ready → 通知前端 → `running`（Rust）
3. 意外退出且 `running` → 退避计数判定 → `restarting` | `restart-stopped`（Rust）
4. 退避延迟到期 → spawn → `first-starting`（Rust）
5. 前端握手成功 → `connected`（前端）
6. 流终止/握手失败 → `reconnecting`（前端）
7. 重连成功 → `connected`（前端）
8. 用户退出（Cmd+Q/托盘）→ `stopping`（Rust）
9. 首次启动失败 → 对话框；重试 → `first-starting`；退出 → `stopping`（Rust+UI）
10. 托盘重试（`restart-stopped`）→ 重置计数 → `first-starting`（Rust）

### 退避规则（可执行定义）

- 任何 sidecar 意外退出（exit code 任意，含 0）触发重启；重启延迟 = `min(30s, 1s × 2^(n−1))`，n = 连续失败序号（1 基）
- 连续失败计数器仅在 sidecar **存活 < 30s** 时 +1；存活 ≥ 30s 的退出重置计数器与延迟
- 计数达 5 → 托盘弹窗 + 停止自动重启；托盘「重试」重置计数并立即重启
- 首次 socket-ready 之前的失败不进入该计数（走对话框）
- 覆盖「启动后立即死亡」：坏 patch/坏插件崩溃循环每轮存活 < 30s → 5 次后停止（总耗时约 15s + 启动时间）
- 两循环衔接：sidecar 死 → WS 断 → 前端代际失败 → 快速失败（ENOENT/ECONNREFUSED）→ 前端退避；同时 Rust 重启 sidecar，socket 回归后前端下次重试命中——无需显式衔接信号；唯一断点是 describe 超时（§4.3 已由前端 unary 超时闭合）

### 错误处理表

| 场景 | 行为 |
|---|---|
| sidecar 启动失败（缺二进制/缺 key 等） | 对话框展示 stderr（tail 20 行），提供重试与退出；不计入崩溃退避计数 |
| sidecar 崩溃 / 意外退出（含 exit 0） | 按退避规则重启（见上）；连续 5 次 → 托盘弹窗 + 停止自动重启 |
| UDS 连接失败 / WS 断线 | 前端 ConnectionController 代际重建（唯一重连所有者）；Rust 侧对未就绪 socket 快速失败 |
| 在途 unary（重建期间） | 不取消：以传输错误 reject，不吞、不自动重放，UI 提供重试 |
| UDS 权限运行时被改 / 外部 connect 被拒 | 握手 403/拒绝 → 前端重试；检测到「connect 成功但握手被拒」→ 弹「socket 权限异常」诊断而非静默重试 |
| 退出超时 | SIGTERM(组) 后 5s 未退 → SIGKILL(组) |
| socket 残留 | carrier 侧 bind 前 connect 探测 + unlink；Rust spawn 前活体探测兜底（活体 → 提示已在运行，不 unlink）；权限不符（0600 失败）→ 启动失败并报错 |
| 磁盘满 | Rust 临时文件写失败 → 明确错误（上传/下载报「磁盘空间不足」）；sidecar 落盘失败走 host 错误路径 |
| sidecar 输出乱码 | `from_utf8_lossy` 解码 + spawn 时显式 `LC_ALL`（见日志策略） |
| WebView 加载失败（fork dist 未打进包） | Rust 检测 frontendDist 缺失/加载失败 → 错误对话框而非白屏；fork 内最小错误页兜底 |
| 大 body（附件上传 160 MiB 上限、`session.export` 流式下载） | **>~10 MiB 一律走临时文件交接**（invoke 大 payload 实测代价高：150MB 响应约 23s）：上传 = 前端写文件 → Rust 读文件转发；下载 = Rust 落盘 → 前端读；纪律见 §4.6；MB 级以下 invoke 直传 |
| 系统睡眠/唤醒 | WS 静默超时断开 → 前端代际重建自然恢复；backoff 定时器晚触发无碍（确认项，无需处理） |

## 7. 测试

- Rust：uplink/downlink 管道单测（mock 一个 UDS HTTP+WS 服务端；含 Channel 顺序、取消传播、路径回退链超长用例、close_stream 幂等、输入校验拒绝）；进程管理单测（假 sidecar：正常退出/崩溃码/挂起/存活 <30s 计数、退出序列取消重启定时器、进程组信号）
- uds-carrier 插件：单测（UDS 请求 → apiProxy 命中、两路 WS 帧、每连接 getpeereid 拒绝、socket 清理、残留 socket 探测 unlink、目录 0700）
- 状态机测试：迁移表 10 条路径（含 first-ready 分界、restart-stopped 重试）
- e2e smoke（`tauri dev`）：起真 sidecar → UDS 连接 → `host.describe` 成功 → WebView 渲染 fork dist → 无 key 时界面就绪；有 `DEEPSEEK_API_KEY` 时可跑一个真实 agent 回合
- 通知：mock notification 插件，验证完成事件触发
- capability：验证 fork 页 invoke 非白名单命令被拒

## 8. 风险与兜底

| 风险 | 兜底 |
|---|---|
| bun compile 的 `import.meta.url` 资源解析行为不符预期 | 薄自定义入口显式设置资源根 |
| `WebSocketDownlinks` 不在 npm 公开导出面（vendor 拷贝） | vendor 拷贝随上游变更手工同步；锁版本 |
| Tauri 事件 emit 乱序 / 大 payload 慢（下行帧序、附件字节） | 下行用 `tauri::ipc::Channel`（顺序保证）；>~10 MiB 走临时文件交接 |
| UDS 路径超长（`sun_path` 104 字节上限） | 三级回退链（`$DSH_HOME/run` → `os.tmpdir()/dsh-<uid>` → `/tmp/dsh-<uid>`）；目录 0700；单测覆盖长路径 |
| 系统 WebView JS 兼容（10.15 Safari 13.1 无 ES2022） | `minimumSystemVersion: "12.0"` |
| code-runtime worker_threads / `node:sqlite` 在 bun 下的兼容性 | 实测；不过则 sidecar 回退为「node 二进制 + 包目录」布局（进程管理不变） |
| 上游 preview 快速迭代 | fork 锁 transport 层；定期合并 |
| `__DSH_BOOT__` 自产与 boot 内核（`boot.tsx`）的对接面 | 复用 `injectBootManifest`；必要时 fork 自组装入口 |
| XSS 经 IPC 升级（capability 误配/误加插件） | §4.5 白名单原则 + CSP + on_navigation + asset scope 四道闸；评审时核对 capability 文件 |
| describe 挂起（sidecar 活但卡死） | 前端统一 unary 超时（含握手 describe） |
| sidecar 子进程孤儿（SIGKILL 路径） | 进程组 spawn + 组信号（§4.2） |
| App Sandbox 引入的 sidecar 不可用 | v1 不做沙箱（记录为已知限制）；hardened runtime + 公证 |

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
2. M2：Rust 哑管道 + 进程管理（含状态机/退避/进程组/退出序列），CLI 下验证 uplink/downlink 转发
3. M3：前端 fork + transport 替换（含 capability 最小集、CSP、unary 超时），`tauri dev` 跑通完整聊天
4. M4：托盘 / 自启 / 通知 / 导航白名单 / 临时文件纪律打磨，打包 `.app`
5. M5：测试补齐 + 文档
