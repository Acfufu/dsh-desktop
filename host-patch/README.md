# dsh-desktop host-patch

DeepSeek Harness 桌面载体 patch 包：禁用全部 TCP 载体行，插入 UDS 载体。

## 组成

- `desktop.patch.yml` — 桌面 patch（`--patch` overlay）
  - 禁用：`webserver`、`web-runtime`、`connection`、`client-hmr`、`modules`、`directory-picker`（TCP 载体行 + 依赖 webServer 的行）
  - 插入：`@dsh-desktop/uds-carrier`（UDS 载体）、`@deepseek-ai/dsh-host-directory-picker-native`（原生目录选择，替代 auto 版）
- `packages/uds-carrier/` — 本地目录插件（不发布 npm）
  - `src/socket-path.ts` — UDS 路径回退链（`$DSH_HOME/run/dsh.sock` → `os.tmpdir()/dsh-<uid>` → `/tmp/dsh-<uid>`，≤100 字节）
  - `src/index.ts` — node:http UDS 服务：uplink `bridge + toFetchHandler(apiProxy)`、downlink vendored `WebSocketDownlinks`、interceptor 双层分发、socket 目录 lstat/owner 校验
  - `vendor/` — 上游 connection 包逐字节拷贝（`sync-carrier.sh` 哨兵防漂移）
- `UPSTREAM_PIN` — 上游 commit 锁定（vendor 拷贝 + npm 依赖同 commit）

## 启动（真实 checkout 验证）

前置：deepseek-harness checkout 已 `pnpm install && pnpm run build`（host+client lib）。

```bash
# 1. 构建插件产物
cd host-patch && pnpm exec tsc -p packages/uds-carrier/tsconfig.build.json
# 2. 装入 profile node_modules（loader 从 $DSH_HOME/profiles/<profile> 起解析裸包名）
DSH_HOME_DIR=/tmp/dsh-m1-test   # 首次运行自动初始化 profile 目录
mkdir -p "$DSH_HOME_DIR/profiles/web/node_modules/@dsh-desktop"
cp -r host-patch/packages/uds-carrier "$DSH_HOME_DIR/profiles/web/node_modules/@dsh-desktop/"
rm -rf "$DSH_HOME_DIR/profiles/web/node_modules/@dsh-desktop/uds-carrier/src" \
       "$DSH_HOME_DIR/profiles/web/node_modules/@dsh-desktop/uds-carrier/vendor" \
       "$DSH_HOME_DIR/profiles/web/node_modules/@dsh-desktop/uds-carrier/tsconfig*.json"
# 3. 启动（无 TCP 监听；--port 是 app 级参数，勿放在 launcher 旗标区）
DSH_HOME="$DSH_HOME_DIR" node ~/codehub/deepseek-harness/apps/cli/lib/bin.js \
  --profile web --patch "$(pwd)/host-patch/desktop.patch.yml" &
# 4. 验收
./scripts/verify-m1.sh "$DSH_HOME_DIR/run/dsh.sock"
```

## 已知偏差（相对计划，实证记录见 docs/m1-spike-findings.md）

1. patch config 中 `${DSH_HOME}` **不展开**（生成字面量目录）——udsPath 留空，由插件读 `process.env.DSH_HOME` 选路径；生产由 Rust 物化绝对路径。
2. `directory-picker`（auto）依赖 webServer 必须禁，改插 `-native`（无 webServer 依赖，macOS osascript）。
3. cordis `Service` 构造即注册服务——`apply()` 不得再 `ctx.provide`（重复注册报错）。
4. pnpm 11：构建脚本白名单在 `pnpm-workspace.yaml` 的 `allowBuilds: { <pkg>@<ver>: true }`（`pnpm` 字段废弃）。
5. `tsconfig.build.json` 需 `rootDir: "."`（产物 lib/src/）且源码相对导入带 `.js` 后缀（node ESM）；package.json `main` 指向 `./lib/src/index.js`。
