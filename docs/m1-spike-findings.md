# M1 Spike Findings — baseUrl 锚定 + Typert 路由缺口

日期：2026-08-16

## 1. baseUrl 锚定结论（决定插件装哪）

实证链（deepseek-harness @ 47f94385 源码）：

1. `apps/cli/src/profile-boot.ts:227` — `rootConfig = join(composed.profile.dir, PROFILE_ROOT_FILENAME)`，即 `$DSH_HOME/profiles/web/cordis.yml`。
2. `packages/boot/app-boot/src/index.ts:769` — `ctx.baseUrl = pathToFileURL(dirname(absoluteConfigPath)).href + '/'` → **baseUrl 锚定 profile 目录 `$DSH_HOME/profiles/web/`**（不是 patch 文件目录）。
3. `vendor/loader/src/config/tree.ts:155-157` — 插件名解析：`internal.import(name, ctx.baseUrl)` 裸包名走 loader 原生助手（从 baseUrl 起做 node_modules 上溯）；相对路径走 `import(new URL(name, baseUrl).href)`（file URL，仅 `.` 开头 specifier）。
4. `packages/boot/app-boot/src/profile.ts` — profile 有独立 `node_modules`（`nodeLinker: hoisted`），且 launcher 维护平铺回退 `$DSH_HOME/profiles/node_modules`（app 依赖闭包每包一条 symlink）。插件裸包名从 profile 目录上溯解析：profile node_modules → profiles/node_modules 回退。

**结论：`@dsh-desktop/uds-carrier` 必须装入 `$DSH_HOME/profiles/web/node_modules/@dsh-desktop/uds-carrier`**（或同目录 symlink），loader 才能解析。其运行时依赖（`@deepseek-ai/dsh-host-apiproxy`、`ws`、`@deepseek-ai/cordis`）经 launcher 平铺回退 `$DSH_HOME/profiles/node_modules` 解析（三者均在 web 组合依赖闭包内）。patch 的 `config.udsPath: ${DSH_HOME}/run/dsh.sock` 展开待 Task 4 实测定案；未展开则 Rust 侧物化兜底（spec §8）。

## 2. Typert 路由缺口（M1 实测确认）

wire 格式实证（facts §2）：`/api/<ns>/<method>` 两段式（如 `/api/commands/execute`），非单段 `/api/commands.execute`。

**实测（desktop 组合，UDS）：** `/api/commands/execute`、`/api/goals/execute`、`/api/plugin/inventory` 均 → HTTP 404。

**根因：** typert HTTP 承载在 connection 包（`rpc.ts` `/api` channel + typert server 绑定 webServer 路由）——desktop 禁用 connection 后 remotes（commands/goals/pluginInventory/dynamicRunner，`packages/api/remotes/src/client/index.ts` 经 `ctx.remote.$mount` 挂入 `ctx.typert.remotes`）失去 HTTP 面。typert registry 服务本身仍在（api-gateway 提供 remote、依赖 typert——boot 无 pending，host.describe 正常）。

**对策（已落地）：** carrier 双层分发——`registerInterceptor/unregisterInterceptor`（claims+fetch），uplink handler 先查 interceptor 再落 bridge；单测覆盖 claims 真/假两分支。M3 前端 fork 的 transport 替换（tauri invoke）或 Rust 侧实现 typert HTTP 承载时，注册 interceptor 即可恢复这些端点。

## 3. 环境侧记

- patch config 中 `${DSH_HOME}` **不展开**——生成字面量目录（实测：cwd 下出现 `${DSH_HOME}/run`）。结论：patch 不写 udsPath，由插件读 `process.env.DSH_HOME`（运行时 JS 侧展开）选路径；生产 Rust 物化绝对路径（spec §8）。
- desktop 组合必须额外禁用 `directory-picker`（auto，源码级 inject webServer）+ 插入 `@deepseek-ai/dsh-host-directory-picker-native`（无 webServer 依赖，osascript 原生选择器；apiproxy 静态 inject directoryPicker，无 provider 则整链不激活）。
- cordis 4.0.1 `Service` 构造即 `ctx.reflect.provide`——`apply()` 再 `ctx.provide` 报「service has been registered」。

- harness `pnpm install` 2m32s + `build:lib:host` 完成（pnpm 11.7.0，corepack 网络恢复后）。
- pnpm 11 新设置位：`allowBuilds: { <name>@<version>: true }` 在 `pnpm-workspace.yaml`（`pnpm` 字段已废弃）。
- cordis 4.0.1 `Service` 构造走 `ctx.reflect.provide`；`Events` 接口无 `dispose` 键（运行时发射）——本仓库以模块增强补类型。
- apiproxy 包自身增强 `Context.apiProxy: ApiProxy`，无需自声明。
