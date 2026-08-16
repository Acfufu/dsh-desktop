# dsh-desktop — Verified Facts & Spec Corrections（实施基线的实证底稿）

> 来源：2026-08-16 对 `~/codehub/deepseek-harness`（HEAD `47f943859bef60e4160492346772ded9b24f765a`, branch `master`）的 6 路并行源码核查 + crates.io/npm/docs.rs 版本实证。
> 本文件是全部 5 个 milestone 计划的共享事实底座。任何计划任务不得引用与本文冲突的符号名。
> 执行者（deepseek-v4-flash 级别）在遇到「与本文不符的上游代码」时，**以本文为准记录偏差，不得自行猜 API**。

## 0. 环境实证（本机）

| 项 | 值 | 与 spec 差异 |
|---|---|---|
| macOS | 26.6.1, arm64 | — |
| Xcode | full Xcode 已装（/Applications/Xcode.app）+ CLT | — |
| rustc/cargo | 1.97.1（Homebrew） | ≥ tauri MSRV 1.77.2 ✓ |
| node | v24.14.1 | 满足 `^22.19.0 || >=24.0.0` ✓ |
| pnpm | 11.3.0 | spec 写 pnpm 9 → **用已装的 11.3.0**（pnpm 向后兼容 workspace 协议） |
| tauri-cli | **未安装** | 每个需要它的里程碑先 `cargo install tauri-cli --locked --version 2.11.4` 或 npm i -D `@tauri-apps/cli@2.11.4` |
| DSH_REPO | `~/codehub/deepseek-harness` 存在 | — |

## 1. Profile / patch 组合（核查 1、5）

- **profile 'web' 不是 web-app patch 定义的**：profile 目录 = `$DSH_HOME/profiles/web`（`packages/boot/app-boot/src/profile.ts:114-117` PROFILE_TEMPLATES，bundles = `['@deepseek-ai/dsh-base', '@deepseek-ai/dsh-web-app']`）。web-app `cordis.patch.yml` 只是 bundle 层 patch。
- **patch 应用顺序（确认）**：bundle(base→web-app) → profile 自身 `cordis.patch.yml` → `$DSH_HOME/cordis.patch.yml` → `--patch` overlays（argv 顺序，`apps/cli/src/args.ts:61,132,163`）→ agent-presets shipped-root（`config.roots` 整键覆盖，`profile-boot.ts:159-167`）→ telemetry switch（`168-169`）。**desktop patch 不是最终层**；telemetry 永远最后。
- `--patch a.yml --patch b.yml` 可重复 ✓。`--port 0` = OS 分配 ✓（`packages/host/webserver/src/index.ts:48-49,62,78-81,216-224`）。**`--host 0.0.0.0` 被 CLI 拒绝**（`packages/bundle/web-app/src/startup.ts:69-71`）——webserver schema 允许 0.0.0.0 但 CLI 不许。
- **`disabled: true` 语义（修正）**：不是 compose 期跳过——行保留在组合树中（`disabled: true` 标记），**激活期**被 loader 跳过（`vendor/loader/src/config/entry.ts:124-128` refresh early-return），且**不产生 `__DSH_BOOT__` 行**（`packages/client/modules/src/index.ts:387` processOne 要求 `!entry.disabled`）。
- web-app patch 的 inject 行（patch 行级）：webserver → `[webStartup]`（:117）、web-runtime → `[webStartup]`（:132）、connection → `[webRuntime]`（:158）。**patch 内没有 `inject: ['webServer']` 行**；webServer 注入在**插件源码级**：connection `['webServer']`（connection/src/index.ts:47）、client-hmr `['clientModules','webServer']`（hmr/src/index.ts:28）、modules `['webServer','loader']`（modules/src/index.ts:185）、web-app 本身 `['webServer']`（bundle/web-app/src/index.ts:35）。→ desktop 禁用 webserver 后 connection/client-hmr/modules 因缺 provider 无法激活，**必禁**成立（机制是源码级 inject，不是 patch 行级）。
- **`__DSH_BOOT__`**：schema `{rev, entries:[{id,url,rev,inject?,immediately?}]}`（`packages/client/modules/src/client/manifest.ts:50-69`，parser :108-144）。生成在 host 侧 `ClientModuleRegistry.compose()`/`graphRow()`（modules/src/index.ts:150-158, 315-318）；`url: \`/plugins/${id}/client.js?rev=${rev}\``（:153）。**`injectBootManifest(html, graph)` 是 `@deepseek-ai/dsh-client-modules` 根导出**（index.ts:168，package.json exports "."）。**`assertEntriesActive` 不存在**——实际失败机制：`arrive()` 抛「loaded without registering」（system.ts:105-107）、`import()` 对缺失行抛错（:170-173）。fork 计划不得写 assertEntriesActive。
- **bundle 数**：39 个包声明 `dsh.client`（29 个 ui-* + 10 个非 ui：typert/registry、session-log-export、cordis-client-runner、api/remotes、api/gateway、locale、connection、hmr、runtime、modules）。web-app patch 共 51 个 insert 行（50 + agent-presets）；browser-roster 段（:151-274）= **33 行**；「~36 client 行」= 35 个 client 命名行 + api-gateway。desktop 禁用 connection/client-hmr/modules 后余 **33 行（spec 口径）**，精确数以运行时组合为准，**禁硬编码**。

## 2. connection 包传输缝（核查 2、3、7）

- `packages/client/connection/src/client/` **只有 7 个文件**（不是 spec 的 13）：api.ts、connection.ts、fixture.ts（**恰好 3188 行**）、index.ts、random-uuid.ts、rpc.ts、web-api-client.ts。
- `index.ts:88`：`const api: IApiClient = fixtureClient ?? new WebApiClient()`——不是硬编码，`?fixture` 是 **apply() 内运行时 URLSearchParams 判断**（:86-89），非 vite 分支。
- `web-api-client.ts`：`doFetch` = 裸 `globalThis.fetch`（:14-16，**无超时**）；`openMux`/`openHost` 是 async iterator，**yield `RpcRequest<MuxFrame|HostFrame>`（`{rpcId, payload}`），不是 ServerRequest**——ServerRequest 只进 `onEnvelope` tap（:62）；WS 仅文本帧（:55 二进制抛错）；无客户端应用消息发送（respond 走 HTTP POST /api/respond）。
- `AbstractApiClient`（`packages/host/apiproxy/src/fetch/client.ts`）：`DEFAULT_TIMEOUT_MS=30_000`（:228）；`caller-signal-only`（:231, 317）；**`host.pickDirectory` 是基类唯一 caller-signal-only**（:434-444）。
- **`command.execute` 不在 unary 面**：RpcMethodMap 无 command.*（`apiproxy/src/api/rpc-map.ts:24-77`）。走 TypertGateway 通道：ui-commands → `connection.rpc.call('/api','commands/execute',...)`（`packages/api/gateway/src/client/index.ts:406`）→ host `connection.rpc.intercept('/api', ...)`（`packages/api/gateway/src/index.ts:105-110`，该插件 `ctx.inject(['connection'])` :104）。
- **ConnectionController 语义全部确认**：任一流结束→代际失败→退避 500ms×2 cap 10s（connection.ts:19-24, 93）；握手 = 双流 open + `host.describe` 成功（:138-155）；`streamOpenTimeoutMs`(3s) **只竞速双流 open，不覆盖 describe**（:141）；abort 只中止两路流，**不取消在途 unary**（:128-129, describe 无 signal :140）。
- **`createWebConnectionRpc` 有消费者（修正 spec「无消费方」）**：`packages/api/gateway/src/client/index.ts:406` 经 `connection.rpc.call('/api',...)` 消费 handle.rpc。形状：`call(channel, endpoint, payload, signal?)` → POST `${channel}/${endpoint}`，ClientRequest 信封，`serverResponseSchema` 解析 + rpcId echo，返回 `full.result`（client/rpc.ts:19-49, 56-63），用 `globalThis.fetch`（:30）。fixture 模式旁路（fixture.ts:2998-3014）。
- `serverRequestSchema` 定义在 `@deepseek-ai/dsh-host-apiproxy` → `packages/host/apiproxy/src/api/rpc.schema.ts:114-119`（type='server-request' + rpcId + method + payload unknown）。

## 3. 导出面 / vendor 清单（核查 3、7）

- **`toFetchHandler(api)`**：`@deepseek-ai/dsh-host-apiproxy` **根导出**（src/index.ts:27；定义 fetch/handler.ts:243），v0.1.0-rc.6。`./client` 子路径 → AbstractApiClient/IApiClient/InProcessApiClient。npm files 含 `lib/types/**/*.js` → **可从 npm 引**。
- **`bridge` + `DEFAULT_MAX_REQUEST_BODY_BYTES`(160 MiB)**：在 connection `http-bridge.ts`（bridge :32，const :12），但**不在包根导出**（connection/src/index.ts:9 只 import 不 re-export），exports map 只有 `.`、`./invariant`、`./client`、`./src/*`、`./package.json`，而 `./src/*` 不在 npm files（files = lib 3 个 js + lib/types/**/*.d.ts）→ **必须 vendor 拷贝（连同 WebSocketDownlinks）**。
- **`WebSocketDownlinks`**：`constructor(api: ApiProxy)`（websocket-downlink.ts:51,56），不依赖 webServer，独立可用；不在导出面、不在 files → **vendor**（spec 已说，确认）。
- webserver：仅 TCP；upgrade 按**精确 pathname** 分发（webserver/src/index.ts:194 `this.upgrades.get(new URL(req.url).pathname)`）；teardown `closeAllConnections()`（:232，Node 不覆盖升级 socket，另有显式 upgradedSockets destroy :226-227）。路径常量在 connection `api-path.ts`：`MUX_EVENTS_PATH='/api/events.mux'`、`HOST_EVENTS_PATH='/api/events.host'`。
- `PRIVILEGED_METHODS` 环回钉死：connection/src/index.ts:89-119（host.pickDirectory/openPath、settings.*、credentials.*、agentPreset.*、llm.discoverModels）。desktop 禁 connection 后此钉死消失 → 由 UDS 文件权限信任取代（spec 已说，确认）。

## 4. **apiProxy 供给缝（M1 命门，核查 8）**

- patch 行 `api-gateway`（cordis.patch.yml:99）= **`@deepseek-ai/dsh-host-apiproxy`**（不是 `@deepseek-ai/dsh-api-gateway`）。它是 **`apiProxy` 服务提供者**：`ApiProxyService extends Service`，`super(ctx, 'apiProxy')`（apiproxy/src/index.ts:97）。
- `apiProxy` 服务的**唯一 host 消费者 = connection/src/index.ts**（:137, 156, 174）。base bundle 另有 `typert-gateway` 行 = `@deepseek-ai/dsh-api-gateway`（TypertGatewayService），它 `ctx.inject(['connection'])` 并 `connection.rpc.intercept('/api', ...)`——**desktop 禁用 connection 后它注入永不 resolve（软依赖，不硬崩），但 typert 远端端点失去分发**。
- **路由缺口（M1 spike 必查）**：浏览器 `remote.commands.*`（ui-commands、ui-plan）、`remote.goals.*`（ui-goal）、`remote.pluginInventory.list`、`remote.dynamicCordisRunner.*`（cordis-client-runner、ui-cordis）经 client 侧 rpc → POST /api/<endpoint> → host 侧需 TypertGateway 拦截器分发；`toFetchHandler(apiProxy)` 的 RpcMethodMap **不含 command.***。carrier 若只接 `toFetchHandler(apiProxy)`，`commands.execute` 等会 method-not-found。→ **M1 必须有 spike 任务实测**：desktop 组合下 POST /api/commands.execute 是否可达；不可达则 carrier 需自实现「interceptor 优先 + apiProxy 兜底」的双层分发（镜像 `connection/src/rpc-host.ts` createSharedFetchHandler 逻辑）或保留一个最小 connection 兼容服务。**此为 M1 验收项 ⑥ 之外的显式决策点**。

## 5. 前端 fork 清单（核查 4）

- `apps/web/src/main.ts` = 10 行壳（import AppWebEntry + mount）；`index.html` 14 行**无 CSP meta**；`vite.config.ts` 有 `rejectStandaloneServe`（:11-19，config hook 里 env.command==='serve' 时 throw）+ **alias 数组直指 workspace src**（:138-149：dsh-client-web→web/src/boot.tsx；web-react/ui-slots/ui-primitives/ui-attachment/schema-form→各 src/index.ts；modules/client→src/client/index.ts；node:module→node-module-stub.ts）+ vendor manualChunks（:41-61,118-125）+ cordis-loader process defines（:151-159）。`public/` = favicon.svg + manifest.webmanifest ✓；`node-module-stub.ts` ✓。
- `packages/client/web/src/` **恰好 13 文件**，清单与 spec 一致。`seed.ts` 静态 import：web-react(:15)、ui-slots(:14)、ui-primitives(:16)、ui-attachment(:17)、schema-form(:18) + react/cordis。
- npm files：web/connection/modules 全部 **lib-only 无 src**；web 的 exports map 有 `./src/*` 但 **npm tarball 不 ship src**（workspace 内才可达）→ **vite 编译必须用 alias 指 workspace 源码，不能靠 npm 依赖编译 src**（spec §4.3「vite 必须编译 src/ 而非 lib」确认）。
- `apps/cli/package.json`：files = `["lib/*.js","config"]`（:17-20）；**engines 不在 apps/cli**——在根 `package.json`：`{"node":"^22.19.0 || >=24.0.0"}`。`INSTALL_ANCHOR`（profile-boot.ts:54，lib/../package.json）、`SHIPPED_PRESET_ROOT`（:35，lib/../config/agent-presets/）。
- 插件加载（bun 不可行实证）：loader = vendored `@deepseek-ai/cordis-plugin-loader`（vendor/loader），`import(name)` → `internal.import(name, baseUrl)` 或 `/* @vite-ignore */` 动态 import（config/tree.ts:145-162）——裸包名运行时计算，bundler 静态不可见；`mountRootInclude`（boot/app-boot/src/index.ts:492-504）；`healProfilesModuleFallback`（profile.ts:223-255，symlink profiles/node_modules）；`resolveBundleDir`（:344-355，缺则抛错）；typert-loader 全磁盘依赖（createRequire/require.resolve/readFileSync）。
- HMR fallback：`runProfile` 在 `ctx.get('hmr')===undefined` 时挂 watch-only `cordis-plugin-hmr`（root:[]，profile-boot.ts:279-284）——web profile 恒触发（hmr 行被禁）→ node 运行时没问题，bun 下炸（spec §2 确认）。
- `process.execPath` spawn 仅 Windows 路径：sandbox-local 的 windows-acl runner（win32 chain，:160-165, 557-564；macOS=seatbelt `sandbox-exec` :548）+ win32-dialog（native-picker.ts:69 `platform==='win32'` 门控）+ test-support/loader-smoke（dev 工具）。**macOS 运行时无 process.execPath spawn** → node 布局安全。
- `node:sqlite`：session-query-sqlite openAt never（base patch :117-121 + web-app patch :30-33），`await import('node:sqlite')` 延迟（schema.ts:52）；`worker_threads`：code-runtime-worker-thread（:9, 378）。

## 6. 版本 pin（2026-08-16 实证）

| 组件 | 版本 | 备注 |
|---|---|---|
| tauri | **2.11.5** | MSRV 1.77.2, edition 2021 |
| tauri-build | **2.6.3** | |
| tauri-plugin-single-instance | **2.4.3** | |
| tauri-plugin-autostart | **2.5.1** | |
| tauri-plugin-notification | **2.3.3** | |
| tauri-plugin-opener | **2.5.4** | |
| tauri-cli / @tauri-apps/cli | **2.11.4** / 2.11.4 | cargo 或 npm 二选一 |
| reqwest | **0.12.28**（推荐，spec 兼容） | `unix_socket` **0.12.23 引入**，**无 cargo feature**（`#[cfg(unix)]` 目标门控，默认 features 即可）；0.13.4 为最新但 breaking（rustls 默认、MSRV 1.85）→ **不用 0.13** |
| tokio-tungstenite | **0.29.0**（R3 修正：crates.io 无 0.29.2，只有 0.29.0/0.30.0；spec 允许 0.28/0.29） | 0.30.0 为最新，spec 未 pin → 用 0.29.0 保守 |
| tokio | **1.53.1** | |
| @tauri-apps/api | **2.11.1** | |
| @tauri-apps/plugin-notification | **2.3.3** | |
| @tauri-apps/plugin-autostart | **2.5.1** | |
| @tauri-apps/plugin-opener | **2.5.4** | |

**Cargo.toml 建议片段（所有里程碑共用）**：
```toml
tauri = { version = "2.11", features = [] }
tauri-build = { version = "2.6", features = [] }
tauri-plugin-single-instance = "2.4"
tauri-plugin-autostart = "2.5"
tauri-plugin-notification = "2.3"
tauri-plugin-opener = "2.5"
tokio = { version = "1.53", features = ["full"] }
tokio-tungstenite = "0.29.0"
reqwest = { version = "0.12.28", default-features = false, features = ["rustls-tls", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

**package.json 建议片段（fork）**：
```json
{
  "@tauri-apps/api": "2.11.1",
  "@tauri-apps/plugin-notification": "2.3.3",
  "@tauri-apps/plugin-autostart": "2.5.1",
  "@tauri-apps/plugin-opener": "2.5.4"
}
```

## 7. Spec 修正汇总（写进计划时生效）

1. `src/client/` 文件数 13 → **7**。
2. `assertEntriesActive` → 不存在；用 `arrive()` 抛错 / `import()` 抛错语义。
3. openMux/openHost yield 的是 `RpcRequest<MuxFrame|HostFrame>`（非 ServerRequest）；ServerRequest 只进 onEnvelope tap。
4. `bridge` + `DEFAULT_MAX_REQUEST_BODY_BYTES` 必须 vendor（非 npm 导出），与 WebSocketDownlinks 同 commit 同 hash 哨兵。
5. `createWebConnectionRpc` 有消费者（gateway client）——不得删除；fork 中换 invoke 版时保持签名 `call(channel, endpoint, payload, signal?)` 与「不设超时」。
6. `engines` 在根 package.json，不在 apps/cli。
7. profile 'web' 定义在 `$DSH_HOME/profiles/web`（PROFILE_TEMPLATES），web-app patch 只是 bundle 层。
8. webServer 注入在插件源码级（connection/hmr/modules 各自 `['webServer'...]`），desktop 必禁理由成立但机制描述修正。
9. pnpm 用 11.3.0（本机），不用 9。
10. **新增缺口**：Typert 远端端点（commands/goals/pluginInventory/dynamicCordisRunner）在 connection 禁用后的分发——M1 spike 决策点（§4 上文）。

## 8. R1 双审修正记录（2026-08-16，5 计划文件应用）

| 编号 | 位置 | 修正 |
|---|---|---|
| R1-1 | M1/M3 启动命令 | `apps/cli/bin.js` → `apps/cli/lib/bin.js`（apps/cli files = lib/*.js） |
| R1-2 | M2 Task 4 | spawn 用 `process_group(0)`（std CommandExt，安全 API）建独立进程组，`graceful_shutdown` 的 `kill(-pid)` 才安全；去掉 pre_exec 悬空承诺 |
| R1-3 | M4 Task 5 | Rust `uds_path` 从 `$DSH_HOME` 派生（缺省 `~/.dsh/run/dsh.sock`），非 `resource_dir/dsh/run`（与 carrier 实际监听路径一致） |
| R1-4 | M2/M3 下行终止 | 统一 `""` 空字符串终止帧：M2 流断/出错时 `send("")`，M3 `text === ''` 判终；删 `dsh:downlink:mux|host` 事件名假设 |
| R1-5 | M3 Task 2 | 导入面拆分：AbstractApiClient/IApiClient ← `/client`；serverRequestSchema ← `/api`；RpcId ← 根；RpcRequest/MuxFrame/HostFrame ← fork `./rpc`（type-only） |
| R1-6 | M3 Task 3 | `postEnvelope` path = `${channel}/${endpoint}`（channel 含前导 /），禁双斜杠 `//api/...`（被 Rust 输入校验拒绝） |
| R1-7 | M4 Task 3 | `subscribeEnvelopes` 未实证 → 主路径改 fork 内 `onEnvelope` tap（facts §2 实证），抽象 `EnvelopeSource` 注入 |
| R1-8 | M4 Task 1 | run() 保留退出序列接线（RunEvent::ExitRequested → prevent_exit → 异步关闭任务），不得被整体替换丢失 |
| R1-9 | M1/M4 | 插件包名与 patch `name` 统一 `@dsh-desktop/uds-carrier`（scoped）；插件入口改编译产物 `lib/index.js`（node ESM 不解析 extensionless） |
| R1-10 | M1 Task 5 | spike wire 格式 `/api/<ns>/<method>`（Typert 两段式）；双层分发（interceptor 优先 + apiProxy 兜底）**在 M1 内落地**，不「下一迭代」 |
| R1-11 | M1 Task 4 | `${DSH_HOME}` 在 patch config 的 env 展开为 spike 验证项（不展开则字面量路径 → 验收②③失败；兜底 Rust 物化） |
| R1-12 | M2 Task 2 | `dsh_http_impl` 纯函数化（薄包装命令），集成测试直测纯函数绕开 State 注入；AppState 三字段完整构造 |
| R1-13 | M2 Task 2 | sidecar 重启后 client 重建（`e.is_connect()` → 重建重试一次，spec §4.2）——补显式任务 |
| R1-14 | M2 Task 3 | `futures-util = "0.3"` 依赖补入；`reader.next()` 需 `StreamExt` |
| R1-15 | M2 Task 4 | `ProbeResult` 三态（Alive/Stale/Error），删空 marker enum；测试并入 process.rs |
| R1-16 | M3 Task 1 | 拷贝包 `workspace:*` 依赖重写为精确版本（standalone pnpm install 才可解析） |
| R1-17 | M3 Task 5/6 | `composed-entries.json` 由 `derive-composed-entries.mjs` 从 bundle 产物派生（读 `dsh.client` + `lib/client.js`），禁手写 33；验收②必须用派生清单 |
| R1-18 | M3 Task 6 | `beforeDevCommand: pnpm --dir ../frontend dev`（dev 必须拉起 vite，否则 devUrl 空白）；dev manifest 真实注入（非 stub） |
| R1-19 | M5 Task 1 | 删 `assert!(true)` 空验证（迁移 7/8 属前端状态机，M3 覆盖）；probe 测试引用确认（M2 已建） |
| R1-20 | M5 Task 5 | 许可逐个从包内 LICENSE 实证，禁断言 |

## 9. R2 双审修正记录（2026-08-16，两审均 FAIL：6+8 个 BLOCKER/HIGH）

| 编号 | 位置 | 修正 |
|---|---|---|
| R2-1 | M1 Task 1 | vendor 文件不再追加头部注释（会破坏 sync-carrier.sh 的 cmp 逐字节校验）——provenance 只放 README |
| R2-2 | M1 Task 2 | `@deepseek-ai/cordis` 显式 devDependency；host-patch 非 workspace，pnpm add 不加 --filter |
| R2-3 | M1 Task 3 | 测试焦点改为 start() 副作用（mock node:fs/node:http）；`this.config` 显式字段赋值；`svc.start().catch(logger.error)` |
| R2-4 | M1 Task 5 | baseUrl 实测**前置于** Task 4 启动验证（插件未入 node_modules 会 DOA）；interceptor 双层分发给完整代码（不再引用上游文件） |
| R2-5 | M2 Task 1 | `frontend/dist` 空占位目录（frontendDist 校验）；`icons/icon.png` Task 1 即生成占位 PNG |
| R2-6 | M2 Task 2 | fake-sidecar 用 `CARGO_MANIFEST_DIR` 锚定绝对路径；AppState 定义提前到 Task 2；`impl Clone`；client 重建测试补代码 |
| R2-7 | M2 Task 3 | `use futures_util::StreamExt` 补入；WS 端到端测试（open_stream 收帧） |
| R2-8 | M2 Task 4 | 迁移 5 语义修正（ever_ready 区分首次/重启后 pre-ready 崩溃）；`ProbeResult` 三态 |
| R2-9 | M2 Task 5 | 150MiB 基准经 `dsh_http_impl`（不经 invoke 不得冒充验收数据点）；退出序列替换 `.run()` 而非追加 |
| R2-10 | M3 Task 1/5/6 | `workspace:*` 重写；node 入口守卫用 `pathToFileURL(argv[1]).href`；doFetch 保留 query string；composed-entries 派生产物；beforeDevCommand 拉 vite |
| R2-11 | M3 Task 2/4 | 删删文件引用 grep 确认；删除自证恒真断言（expect(true)） |
| R2-12 | M3 Task 5 | CSP 注明 dev 变体需放行 HMR ws；build-pipeline 测试依赖 M4 产物已标注 |
| R2-13 | M4 Task 1/4.5/5 | `shutdown_sequence` 完整定义（M4 Task 1 引用）；`nanoid` 提为生产 pub fn；fs/PermissionsExt 导入；tempfiles 补 `use std::fs`；新增 Task 4.5 日志模块（§4.2：轮转/LC_ALL/from_utf8_lossy/panic hook）+ 错误对话框（eval alert，无 dialog 插件） |
| R2-14 | M4 Task 4 | >10MiB 拖拽链路：新增 `dsh_import_dropped` + on_drag_drop_event；下载改 Rust 侧流式落盘（`dsh_export_session`），禁 invoke bytes 回传 |
| R2-15 | M5 Task 1/2 | capability 测试真扫 fork 源码 import；stale unlink 测试驱动真实 `start()`；删 `assert!(true)` 空验证 |
| R2-16 | M5 Task 4.5 | 新增 `scripts/dev.sh`（spec §9 脚本清单补全）+ 握手 describe 10s 调用点超时 + 测试 |

## 10. R3 双审修正记录（2026-08-16，两审 FAIL——B 审做了实证验证）

| 编号 | 位置 | 修正 |
|---|---|---|
| R3-1 | facts §6 / M2 Cargo | tokio-tungstenite **0.29.2 不存在**（crates.io 只有 0.29.0/0.30.0）→ pin `0.29.0` |
| R3-2 | M1/M3 | `@deepseek-ai/cordis` 版本是 **4.0.1**（vendor/cordis/package.json 实证），非 0.1.0-rc.6 → 全部改用 4.0.1 |
| R3-3 | M1 Task 2 | 首次 vitest 前必须 `pnpm install`（Task 1 Step 7 补）；`ws @types/ws` 一并安装（tsc 会类型检查 vendored websocket-downlink.ts） |
| R3-4 | M1 Task 3 | `lib/index.js` 从无构建步骤（tsc --noEmit 只查类型）→ Task 3 Step 6 后加 `pnpm run build`；否则 M1 Task 4 真实启动 ERR_MODULE_NOT_FOUND |
| R3-5 | M1 Task 3/5 | `vi.mock('node:fs')` 缺 mkdtempSync/rmSync/statSync → mock 补全（M1 Task 3 Step 2 + M5 Task 2 Step 2 调用它们） |
| R3-6 | M1 Task 4 Step 6 | WS 探测脚本 `import { connect } from 'node:http'` **不存在**（实证 http.connect undefined）→ 改 `http.request` + 'upgrade' 事件 |
| R3-7 | M1 Task 4 Step 7 | `kill %1 %2` 跨 shell 失效 → PID 捕获 `$!`；同 M2 Task 3 Step 6、M2 Task 5 Step 1 |
| R3-8 | M1 Task 4/5 | `${DSH_HOME}` 展开 + Task 5 重排仍是 note-only → Task 4 头部加显式重排标记；Task 4 Step 1 加 socket 路径实测 |
| R3-9 | M2 Task 1 | printf PNG **损坏**（实证：file/sips 读头 OK 但 zlib 解压 CRC 失败；Rust image crate 解码必失败）→ 用 base64 已验证 1×1 PNG |
| R3-10 | M2 Task 2 | AppState + streams 前引用 Task 3 → AppState+impl Clone+StreamRegistry 完整代码移入 Task 2；`dsh_http_impl` 从未给代码 → 补全 |
| R3-11 | M2 Task 3 | `client_async_tls_with_config` 在默认 features 下**不存在**（实证 0.29 lib.rs cfg 门控）→ 用 `client_async_with_config(request, stream, None)`（非 TLS） |
| R3-12 | M2 Task 3 | `reg.tasks.insert` Mutex 无 insert 方法 → `reg.tasks.lock()?.insert(...)` |
| R3-13 | M2 Task 3 | fake-sidecar 的 `ws` import 无法解析（repo 根无 node_modules）→ repo 根 `npm i -D ws`（或 spawn cwd=frontend） |
| R3-14 | M2 Task 5 | `cargo test bench_big_body` 零测试静默假过 → 补 bench 测试完整代码或改用真实测量命令 |
| R3-15 | M3 Task 1 | `frontend/tsconfig.json` 只列名未给内容 → 补全（ESNext/bundler/strict/paths 8 alias） |
| R3-16 | M3 Task 1 | 6 个 `@deepseek-ai/dsh-client-*` 依赖缺 frontend/package.json → 精确版本补入（seed.ts 静态 import 面） |
| R3-17 | M3 Task 4 | vitest include 只 `packages/**` → scripts/ 测试全被排除 → include 加 `scripts/**/*.test.ts` |
| R3-18 | M3 Task 5 | build-pipeline.test.ts `__dirname` 在 ESM 未定义（R2 只标了 M4 依赖没修 __dirname）→ fileURLToPath(import.meta.url) |
| R3-19 | M3 Task 5 | transformIndexHtml 返回 `{html}` 类型不合法 → `{ html, tags: [] }` |
| R3-20 | M3 Task 5/6 | derive-composed-entries 输入是 M4 产物（里程碑倒挂）→ M3 阶段指向 `~/codehub/deepseek-harness/node_modules` |
| R3-21 | M3 Task 2 | 生产 `(err as any).kind` + `RpcRequest<any>` → Error 子类带 kind 字段；onmessage 内 parse 加 try/catch（防单帧异常杀 generator） |
| R3-22 | M4 Task 1 | `shutdown_sequence` 定义在 Task 5 → Task 1 给可编译 stub（或前移定义） |
| R3-23 | M4 Task 1 | `default_window_icon().unwrap()` 恒 None（conf 无 bundle.icon）→ conf 加 `bundle.icon: ["icons/icon.png"]`；tray.rs 用 if let Some |
| R3-24 | M4 Task 4.5 | `setup_panic_hook`/`tail20` 从未定义；run() 内 `?` 非法 → 补定义 + Result 路径 |
| R3-25 | M4 Task 5 | `ProcessManager` 全无实现（M2 只有纯函数）→ 新增 Task：ProcessManager struct + spawn/watch/backoff 循环 + cancel token |
| R3-26 | M4 Task 4 | `dsh_export_session` 引用了未实现 → 补命令定义（Rust 侧流式拉取→落盘）或从能力清单删除 |
| R3-27 | M4 Task 3 | `ServerRequest` 不在 apiproxy 根导出（实证在 /api）→ import 自 `@deepseek-ai/dsh-host-apiproxy/api` |
| R3-28 | M5 Task 1 | e2e_smoke.rs 无 DSH_SOCKET 时 bare `cargo test` 失败 → 无 env 时 early return skip |
| R3-29 | M5 Task 2 | capability.test.ts `__dirname` ESM 未定义 → import.meta.url |
| R3-30 | M5 Task 4.5 | describe-timeout 测试挂死（mock 忽略 signal，promise 永不 reject）→ mock 监听 abort 后 reject |

## 11. R4 双审修正记录（2026-08-16，安全/生命周期两审均 FAIL）

| 编号 | 位置 | 修正 |
|---|---|---|
| R4-1 | M2 state_machine | `transition` 增 `alive_secs` 参数——≥30s 重置规则可经状态机路径表达（不再硬编码 on_exit(0)）；补 `long_lived_crash_resets_via_transition` 测试 |
| R4-2 | M4 ProcessManager | **watch/backoff 循环完整实现**（此前是 `Ok(())` 骨架）：probe→Alive→退出 / Stale→unlink→spawn；ever_ready 字段（迁移 5）；child.wait→on_exit(alive_secs)→退避 sleep（select! 监听 cancel token）→respawn；RestartStopped→托盘弹窗 |
| R4-3 | M4 module path | `crate::process::ProcessManager` → `crate::process_manager::ProcessManager`（编译错误）；`default_socket_path` 移入 process.rs（被 process_manager 引用） |
| R4-4 | M4 tempfiles | `dsh_import_dropped` 加**拖拽来源白名单**（DROPPED_PATHS HashSet，on_drag_drop_event 记录）——XSS 不能 invoke 任意路径读文件；加 160MiB 大小上限 |
| R4-5 | M4 tempfiles | `dsh_export_session` **完整命令定义**（此前只引用未实现）：Rust 侧拉 session.export → ~/Downloads → 通知；磁盘满 → 「磁盘空间不足」明确消息（dsh_write_temp 同） |
| R4-6 | M4 tempfiles | **age_sweep**（启动按年龄清扫孤儿，spec §4.6）+ 退出序列⑤ remove_dir_all(temp-uploads)（此前只出现在 Interfaces） |
| R4-7 | M3 Task 6 | **组合矩阵 UI**（spec §6）：StatusIndicator 组件——启动中/重连>30s banner+诊断/restart-stopped 托盘弹窗（此前零任务） |
| R4-8 | M5 m6 | 测试改名语义：Running 状态幂等保持（不冒充迁移 6/7/8，后者属前端 ConnectionController） |
| R4-9 | M4 Task 1 stub | shutdown_sequence 可编译 stub（Task 1）→ Task 4.75/5 换完整版（取消定时器→SIGTERM→5s→SIGKILL→unlink→exit） |

## 12. R5 双审修正记录（2026-08-16，综合终审两审 FAIL——B 审做了实证：npm/pnpm/上游源码）

| 编号 | 位置 | 修正 |
|---|---|---|
| R5-1 | facts §6 / M1 / M3 | **`@deepseek-ai/dsh-*` 版本 0.1.0-rc.5 未发布**（npm 实证：0.0.1-rc.5/0.1.0-rc.2/rc.3/rc.6）→ 全部 pin `0.1.0-rc.6`（含 facts §6、M1 Task 2、M3 Task 1/5/6） |
| R5-2 | M3 Task 5 / M4 Task 5 | **pnpm workspace 的 root node_modules 不含 @deepseek-ai/\***（实证 `~/codehub/deepseek-harness/node_modules/@deepseek-ai` 不存在）→ derive-composed-entries 输入改 `~/codehub/deepseek-harness/node_modules` 仍为空 → 改从 `apps/cli` 闭包/pnpm deploy 取；build-sidecar 用 `pnpm deploy --legacy` 产封闭 node_modules |
| R5-3 | M1 Task 2 | uds-carrier/tsconfig.json 列了 Files 无步骤创建（tsconfig.build.json extends 失败）→ Step 5 补全内容 |
| R5-4 | M2 Task 2/3 | AppState 双定义（http_command.rs + lib.rs）E0308 → **AppState 唯一真源在 lib.rs**（Task 2 Step 1 建 mod 声明 + AppState），http_command.rs 只 `use crate::AppState` |
| R5-5 | M2 Task 2 | mod 声明缺失 → cargo test 静默跑 0 个 → Task 2 Step 1 建 lib.rs mod 声明 |
| R5-6 | M3 Task 5/1 | devBootManifest 无 `apply:'serve'` → pnpm build 也触发 transformIndexHtml 读不存在文件 → 补 `apply: 'serve'` |
| R5-7 | M3 Task 5/6 | dev 模式 plugin bundle 无供给（/plugins/<id>/client.js 404）→ Step 8.5 拷 public/plugins |
| R5-8 | M3 Task 1 | fork 依赖面不全（web/src 还 import ui-theme/runtime/app-shell/invariants）→ 补 4 个 deps |
| R5-9 | M3 Task 2 | abortHandler 在 invoke 后才注册（signal 挂起期触发永不处理）→ invoke 前注册 + invoke 后查 signal.aborted |
| R5-10 | M4 Task 4.75 | **ProcessManager::start 从未被调用**（app 永不 spawn sidecar）→ run() 补 .setup 调 start（Arc manage）；`tokio::spawn` 需 'static → self 改 `Arc<Self>`，闭包内 clone Arc |
| R5-11 | M5 Task 3/4.5 | e2e/dev 脚本预启 sidecar 与 App 自身 ProcessManager 冲突（probe Alive → 退出）→ release/dev 均不再预启；release 断言 app 自产 socket |
| R5-12 | M2 Task 5 | bench 测试无代码（cargo test bench_big_body 静默 0 测试）→ 补完整 bench_test.rs（bench_150mib_through_pipe）+ bench-big-body.mjs 前置 |
| R5-13 | M1 Task 4 / M3 Task 6 | sidecar 后台 `&` 未捕获 $!（kill $SIDE_PID 引用未赋值变量）→ 补 `SIDE_PID=$!`/`M3_SIDE_PID=$!` + Step 7 清理；M3 tauri dev 必须 cwd=src-tauri（beforeDevCommand 相对它解析） |
| R5-14 | M2 Task 1 | `mkdir -p ../frontend/dist` 从 repo 根建到仓库外 → 改 `mkdir -p frontend/dist` |
| R5-15 | M4 Task 1/2 | conf `app.windows` 与 builder 创建窗口冲突（duplicate label panic）→ Task 1 即删 conf windows |
| R5-16 | M4 Task 4.75 | shutdown_sequence try_state 类型 → `Arc<ProcessManager>` |
| R5-17 | M4 Task 5 | build-sidecar 幂等（cp -r 嵌套 node_modules/node_modules）+ node_modules 来源修正（见 R5-2） |

## 13. DeepSec L3 安全审计修正记录（2026-08-16，三轴并行审计：威胁模型 / XSS→IPC 链 / 供应链+构建；B/C 审做了实证）

> **spec 同步**：本记录全部修正已同步进 `docs/2026-08-16-dsh-desktop-design.md` v0.7（§3/§4.1-4.7/§6/§7/§8/M4 验收）；本 facts 文档与 5 个 plan 文件同步修订（提交 fbc951d，spec v0.7 提交 4f8369b）。执行计划时以 v0.7 为基线。

### CRITICAL
| 编号 | 位置 | 修正 |
|---|---|---|
| DS-1 | M4 ProcessManager watch loop | **持 child Mutex 跨 `wait().await` → `take_child` 死锁 → 退出序列卡死 → sidecar 孤儿常驻**。改为锁内 `take()` 出 child 再 wait（wait 期间不持锁） |
| DS-2 | M2 capability / spec §4.5 | **Tauri v2 的 capability ACL 不门控 app 自定义命令**（invoke_handler 命令对全部窗口可调，官方文档+GHSA-57fm-592m-34r7 实证）——spec 把 ACL 当 XSS 闸门的前提错误。修正：XSS 闸门 = CSP script-src nonce + on_navigation + asset scope；capability 只挡插件命令。M5 测试注明 |

### HIGH
| 编号 | 位置 | 修正 |
|---|---|---|
| DS-3 | M3 Task 1 | **`@deepseek-ai/dsh-client-app-shell` 是捏造依赖**（npm 404 实证；上游无此包，APP_SHELL_ID 只是 manifest entry id 字符串）——删除；否则 install 失败 + 可被抢注 |
| DS-4 | M4 Task 5 build-sidecar | **`$ROOT` 未定义**（deploy 落到 /tmp-cli-deploy，`\|\| true` 吞错后回退空 node_modules）——定义 ROOT |
| DS-5 | M4 Task 5 build-sidecar | **pnpm deploy 对 link: override（cosmokit/schemastery）生成指向构建机 workspace 的绝对符号链接**（实证）——`cp -rL` 解引用 + 符号链接残留校验；否则本机验证通过、用户机 MODULE_NOT_FOUND |
| DS-6 | M4 Task 4.75 setup | **$DSH_HOME/.env 从不 chmod → DEEPSEEK_API_KEY 多用户可读**——首次启动 chmod 700 $DSH_HOME + 600 .env |
| DS-7 | M4 Task 2 navigation | **字符串前缀匹配绕过**（tauri://localhost.evil.com / ipc.localhost.evil.com / 127.0.0.1:14200）——改 URL 语义比较（scheme+host 精确 + dev port 精确）+ 对抗测试 |
| DS-8 | M3 Task 5 | **CSP 自锁**：script-src 'self' 禁内联，而 injectBootManifest 注入内联 __DSH_BOOT__ → boot 失败或被迫加 unsafe-inline。加 `script-src 'self' 'nonce-dshboot'` + boot 脚本带 nonce（或实证外链则移除 nonce；二选一断言进管线测试） |
| DS-9 | M3 Task 5/1 | **JSON.stringify 不转义 </script>**——manifest 注入脚本转义 `<` 为 \u003c（纵深） |
| DS-10 | M2 dsh_http / M4 dsh_write_temp | **无 body 上限 → XSS OOM/disk-fill DoS**——两处加 160 MiB 上限 |

### MEDIUM
| 编号 | 位置 | 修正 |
|---|---|---|
| DS-11 | M1 Task 3 | **recursive mkdir 跟随预置 symlink + 无 owner 校验**（共享 /tmp 路径可被其他 uid 预置 → socket 落攻击者目录 → MITM）——mkdir 后 lstat 验证非 symlink 且 uid 匹配 |
| DS-12 | M1 Task 3 | upgrade 回调 `new URL(req.url)` 无 try/catch → 畸形 URL 崩溃 carrier——try/catch 后 destroy |
| DS-13 | M2 graceful_shutdown | **kill(-pid) PID 复用竞态**（sidecar 崩溃后 pid 被复用 → 误杀无关进程组）——kill(pid,0) 先探测；SIGKILL 后 wait 加 10s 超时防挂死 |
| DS-14 | M2 dsh_http | reqwest 默认跟随重定向（可被侧车响应驱动到非 /api 路径）——`redirect::Policy::none()` |
| DS-15 | M3 rpc.ts | JSON.parse + 下游 Object.assign 经 __proto__ 键原型污染——剥除 __proto__/constructor/prototype 键 |
| DS-16 | M4 logging | sidecar.log 默认 0644 其他用户可读——OpenOptionsExt::mode(0o600) |
