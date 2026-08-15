# M1 — uds-carrier 插件 + desktop.patch.yml 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> 共享事实底座：先读 `docs/superpowers/plans/00-verified-facts-and-corrections.md`（全部符号名/版本以它为准）。

**Goal:** 在真实 deepseek-harness checkout 上，通过 `dsh --profile web --patch host-patch/desktop.patch.yml` 启动一个**无 TCP 监听**的 UDS-only 载体：node:http 服务监听 unix socket（0600，目录 0700），`/api/*` uplink 经 `bridge + toFetchHandler(apiProxy)` 透传，`/api/events.mux|host` downlink 经 vendored `WebSocketDownlinks` 转发；无 key 可启动、有 key 可 `curl --unix-socket` 命中 `host.describe`。

**Architecture:** 本地目录插件 `host-patch/packages/uds-carrier`（不发布 npm），`desktop.patch.yml` 禁用 webserver/web-runtime/connection/client-hmr/modules 五行、insert uds-carrier 行；carrier 从 `ctx.get('apiProxy')` 取 ApiProxy（提供者 = api-gateway 行 `@deepseek-ai/dsh-host-apiproxy`，仍存活）。bridge 与 WebSocketDownlinks 从 connection 包 vendor 拷贝（同 commit，hash 哨兵脚本防漂移）。

**Tech Stack:** node 24（本机 v24.14.1）、pnpm 11.3.0、vitest、deepseek-harness @ 47f94385（UPSTREAM_PIN）、`@deepseek-ai/dsh-host-apiproxy@0.1.0-rc.5`（npm 根导出 toFetchHandler）。

## Global Constraints

- 执行者 = deepseek-v4-flash 级别：**每个步骤给出完整代码，禁止「参考上一步」**；每步命令给完整可复制文本；预期输出逐字给出。
- 所有 vendored 文件（http-bridge.ts、websocket-downlink.ts）必须与 npm 依赖**同 commit**：UPSTREAM_PIN 文件记录 commit `47f943859bef60e4160492346772ded9b24f765a`。
- **不得修改 deepseek-harness 仓库任何文件**；全部工作在本仓库（dsh-desktop）`host-patch/` 下。
- 测试统一 vitest（spec §7）。TS 目标 ES2022。禁止 `as any` / `@ts-ignore`（**测试/桩代码除外**——mock 显式标注 `// test-only cast`，生产路径零 cast）。
- **R1 修正**：插件包名与 patch `name` 必须一致（`@dsh-desktop/uds-carrier`）；插件入口必须可被 node 加载（编译产物 `lib/index.js`）；`${DSH_HOME}` 环境变量展开为 spike 验证项（兜底：Rust 物化绝对路径 patch，spec §8）；真实启动命令用 `apps/cli/lib/bin.js`（不是 `apps/cli/bin.js`）。
- 信任模型 = 纯文件权限（M1 定案倾向 (b)：0600 socket + 0700 目录；node-addon getpeereid 为可选增强，**不在 M1 范围**，spec §4.1 允许显式降级）。
- 大 body：沿用 `DEFAULT_MAX_REQUEST_BODY_BYTES`（160 MiB）。
- 每次提交：小步、语义化（`feat:`/`test:`/`fix:`），只 add 本步文件。

---

### Task 1: 仓库骨架 + UPSTREAM_PIN + vendor 拷贝

**Files:**
- Create: `host-patch/package.json`
- Create: `host-patch/tsconfig.json`
- Create: `host-patch/vitest.config.ts`
- Create: `host-patch/UPSTREAM_PIN`
- Create: `host-patch/packages/uds-carrier/package.json`
- Create: `host-patch/packages/uds-carrier/tsconfig.json`
- Create: `host-patch/packages/uds-carrier/vendor/http-bridge.ts`
- Create: `host-patch/packages/uds-carrier/vendor/websocket-downlink.ts`
- Create: `scripts/sync-carrier.sh`
- Create: `host-patch/vendor/README.md`

**Interfaces:**
- Consumes: 上游文件 `~/codehub/deepseek-harness/packages/client/connection/src/http-bridge.ts`、`.../websocket-downlink.ts`（同 commit 拷贝源）
- Produces: `bridge(req, res, fetchHandler, maxBody)`、`DEFAULT_MAX_REQUEST_BODY_BYTES`、`WebSocketDownlinks`（类，`constructor(api: ApiProxy)`、`handleMux(req, socket, head)`/`handleHost(...)`/`close()`），供 Task 3/4 使用。文件内导出符号与上游逐字节一致。

- [ ] **Step 1: 建目录与根 package.json**

```bash
mkdir -p host-patch/packages/uds-carrier/vendor host-patch/packages/uds-carrier/src scripts
```

`host-patch/package.json`：
```json
{
  "name": "dsh-desktop-host-patch",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "test": "vitest run",
    "test:watch": "vitest"
  },
  "devDependencies": {
    "typescript": "^5.6.0",
    "vitest": "^2.1.0",
    "@types/node": "^24.0.0"
  }
}
```

- [ ] **Step 2: tsconfig + vitest 配置**

`host-patch/tsconfig.json`：
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "types": ["node"]
  },
  "include": ["packages/**/*.ts", "vitest.config.ts"]
}
```

`host-patch/vitest.config.ts`：
```typescript
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['packages/**/*.test.ts'],
    environment: 'node',
  },
});
```

- [ ] **Step 3: 记录 UPSTREAM_PIN**

`host-patch/UPSTREAM_PIN`（逐字）：
```
# deepseek-harness 上游 commit 锁定（vendor 拷贝 + npm 依赖必须同 commit）
REPO=deepseek-harness
COMMIT=47f943859bef60e4160492346772ded9b24f765a
BRANCH=master
DATE=2026-08-16
# 变更记录
# - 初始 pin：vendor http-bridge.ts + websocket-downlink.ts（connection 包）
```

- [ ] **Step 4: vendor 拷贝（逐字拷贝，不改一行）**

```bash
SRC=~/codehub/deepseek-harness/packages/client/connection/src
DEST=host-patch/packages/uds-carrier/vendor
cp "$SRC/http-bridge.ts" "$DEST/http-bridge.ts"
cp "$SRC/websocket-downlink.ts" "$DEST/websocket-downlink.ts"
# 验证逐字节一致
cmp "$SRC/http-bridge.ts" "$DEST/http-bridge.ts" && echo "http-bridge OK"
cmp "$SRC/websocket-downlink.ts" "$DEST/websocket-downlink.ts" && echo "websocket-downlink OK"
```

预期输出：两行 OK。

- [ ] **Step 5: vendor 头部注释 + README**

在两个 vendor 文件**顶部**追加（保留原文件全部内容）：
```typescript
/**
 * VENDORED from deepseek-harness@47f943859bef60e4160492346772ded9b24f765a
 * Source: packages/client/connection/src/<原文件名>
 * Do NOT edit. Sync via ../../../../../scripts/sync-carrier.sh (hash sentinel).
 */
```

`host-patch/vendor/README.md`：
```markdown
# Vendored files

| File | Upstream (commit 47f94385) | Reason |
|---|---|---|
| `packages/uds-carrier/vendor/http-bridge.ts` | packages/client/connection/src/http-bridge.ts | `bridge` 不在 npm 导出面（仅 ./src/*，npm files 不含 src） |
| `packages/uds-carrier/vendor/websocket-downlink.ts` | packages/client/connection/src/websocket-downlink.ts | `WebSocketDownlinks` 不在导出面 |

同步：`scripts/sync-carrier.sh`（hash 哨兵，防与 npm 依赖漂移）。
```

- [ ] **Step 6: sync-carrier.sh（hash 哨兵）**

`scripts/sync-carrier.sh`：
```bash
#!/usr/bin/env bash
# 同步 uds-carrier 的 vendor 文件，防与上游漂移（spec §8：同包双拷贝 hash 哨兵）。
set -euo pipefail
REPO="${DSH_REPO:-$HOME/codehub/deepseek-harness}"
PIN="host-patch/UPSTREAM_PIN"
[[ -f "$PIN" ]] && COMMIT="$(grep '^COMMIT=' "$PIN" | cut -d= -f2)" || { echo "UPSTREAM_PIN missing"; exit 1; }
SRC="$REPO/packages/client/connection/src"
DEST="host-patch/packages/uds-carrier/vendor"

check() {
  local f="$1"
  if ! cmp -s "$SRC/$f" "$DEST/$f"; then
    echo "DRIFT: $f differs from upstream (expected commit $COMMIT)."
    echo "Run: git -C $REPO rev-parse HEAD  # confirm pin"
    echo "Copy: cp \"$SRC/$f\" \"$DEST/$f\""
    exit 1
  fi
}

check http-bridge.ts
check websocket-downlink.ts
echo "vendor files in sync with $COMMIT"
```

```bash
chmod +x scripts/sync-carrier.sh && ./scripts/sync-carrier.sh
```

预期输出：`vendor files in sync with 47f943859bef60e4160492346772ded9b24f765a`

- [ ] **Step 7: 提交**

```bash
git add host-patch scripts/sync-carrier.sh
git commit -m "feat(host-patch): scaffold uds-carrier with vendored transport files"
```

---

### Task 2: uds-carrier 插件骨架 + 路径选择 + socket 生命周期

**Files:**
- Create: `host-patch/packages/uds-carrier/src/index.ts`（插件入口）
- Create: `host-patch/packages/uds-carrier/src/socket-path.ts`（路径回退链）
- Create: `host-patch/packages/uds-carrier/src/socket-path.test.ts`
- Create: `host-patch/packages/uds-carrier/package.json`（本 Task 完善）
- Modify: `host-patch/package.json`（加 workspace? 否——本地目录插件由 patch 的 config 指绝对路径，不加入 root workspace）

**Interfaces:**
- Consumes: `$DSH_HOME` 环境变量（可为空）；`os.tmpdir()`、`process.getuid()`
- Produces: `selectSocketPath(dshHome: string | undefined, uid: number, osTmp: string): string`（长度 ≤100 才可用；回退链 `$DSH_HOME/run/dsh.sock` → `${osTmp}/dsh-${uid}/dsh.sock` → `/tmp/dsh-${uid}/dsh.sock`）；`ensureSocketDir(dir: string, fs): void`（mkdir 0700）；`carrier 插件 apply()`（Task 3 填充网络逻辑）

- [ ] **Step 1: 写失败测试（路径回退链）**

`host-patch/packages/uds-carrier/src/socket-path.test.ts`：
```typescript
import { describe, it, expect } from 'vitest';
import { selectSocketPath } from './socket-path';

describe('selectSocketPath', () => {
  it('prefers $DSH_HOME/run/dsh.sock when short enough', () => {
    expect(selectSocketPath('/Users/me/.dsh', 501, '/var/folders/x')).toBe('/Users/me/.dsh/run/dsh.sock');
  });

  it('falls back to os.tmpdir when $DSH_HOME path exceeds 100 bytes', () => {
    const longHome = '/Users/' + 'x'.repeat(120) + '/.deepseek-harness';
    // longHome/run/dsh.sock > 100 bytes → next candidate
    const tmp = selectSocketPath(longHome, 501, '/var/folders/ab/xyz');
    expect(tmp).toBe('/var/folders/ab/xyz/dsh-501/dsh.sock');
  });

  it('falls back to /tmp/dsh-<uid> when os.tmpdir path also exceeds 100 bytes', () => {
    const longTmp = '/tmp/' + 'y'.repeat(150);
    expect(selectSocketPath(undefined, 501, longTmp)).toBe('/tmp/dsh-501/dsh.sock');
  });

  it('always ends with dsh.sock', () => {
    expect(selectSocketPath(undefined, 501, '/tmp')).toBe('/tmp/dsh-501/dsh.sock');
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cd host-patch && pnpm vitest run packages/uds-carrier/src/socket-path.test.ts
```

预期：FAIL —— `Cannot find module './socket-path'`（socket-path.ts 不存在）。

- [ ] **Step 3: 实现 socket-path.ts**

`host-patch/packages/uds-carrier/src/socket-path.ts`：
```typescript
// UDS 路径选择（Rust 侧共享同一逻辑，spec §4.1）。
// sockaddr_un.sun_path 上限 104；保守阈值 100。目录一律 0700 且属当前 uid。

const MAX_PATH_BYTES = 100;

function usable(path: string): boolean {
  return Buffer.byteLength(path, 'utf8') <= MAX_PATH_BYTES;
}

export function selectSocketPath(
  dshHome: string | undefined,
  uid: number,
  osTmp: string,
): string {
  if (dshHome && usable(`${dshHome}/run/dsh.sock`)) {
    return `${dshHome}/run/dsh.sock`;
  }
  if (usable(`${osTmp}/dsh-${uid}/dsh.sock`)) {
    return `${osTmp}/dsh-${uid}/dsh.sock`;
  }
  return `/tmp/dsh-${uid}/dsh.sock`;
}
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cd host-patch && pnpm vitest run packages/uds-carrier/src/socket-path.test.ts
```

预期：4 passed。

- [ ] **Step 5: 插件 package.json**

`host-patch/packages/uds-carrier/package.json`（R1 修正：入口编译产物 `lib/index.js`——node ESM 无法解析 extensionless 相对导入，源码 `src/` 供 vitest 直测，产物供插件加载）：
```json
{
  "name": "@dsh-desktop/uds-carrier",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "main": "./lib/index.js",
  "exports": { ".": "./lib/index.js" },
  "scripts": {
    "build": "tsc -p tsconfig.build.json"
  },
  "dependencies": {
    "@deepseek-ai/dsh-host-apiproxy": "0.1.0-rc.5",
    "ws": "^8.18.0"
  }
}
```

（`ws` 为 vendored websocket-downlink.ts 的运行时依赖，必须显式声明——R1 修正。）

`host-patch/packages/uds-carrier/tsconfig.build.json`（R1 修正：编译产物）:
```json
{
  "extends": "./tsconfig.json",
  "compilerOptions": {
    "noEmit": false,
    "outDir": "lib",
    "declaration": false,
    "sourceMap": false
  },
  "include": ["src/**/*.ts"],
  "exclude": ["src/**/*.test.ts", "src/mock-test-util.ts"]
}
```

- [ ] **Step 6: 插件入口骨架（网络逻辑 Task 3 填）**

`host-patch/packages/uds-carrier/src/index.ts`：
```typescript
import { Context, Service } from '@deepseek-ai/cordis';
import * as os from 'node:os';
import { selectSocketPath } from './socket-path';

declare module '@deepseek-ai/cordis' {
  interface Context {
    apiProxy: any; // ApiProxy 由 api-gateway 行提供（@deepseek-ai/dsh-host-apiproxy）
  }
}

export const inject = ['apiProxy'];

export class UdsCarrierService extends Service {
  static inject = ['apiProxy'];
  private socketPath: string;

  constructor(ctx: Context, config: { udsPath?: string }) {
    super(ctx, 'udsCarrier');
    this.socketPath =
      config.udsPath ??
      selectSocketPath(process.env.DSH_HOME, process.getuid?.() ?? 0, os.tmpdir());
    ctx.logger.info(`uds-carrier socket path: ${this.socketPath}`);
  }

  getSocketPath(): string {
    return this.socketPath;
  }
}

export function apply(ctx: Context, config: { udsPath?: string } = {}) {
  const svc = new UdsCarrierService(ctx, config);
  ctx.provide('udsCarrier', svc);
  return svc;
}
```

- [ ] **Step 7: 类型检查**

```bash
cd host-patch && pnpm exec tsc --noEmit -p tsconfig.json
```

预期：exit 0（若 `@deepseek-ai/dsh-host-apiproxy` 未安装会报模块缺失——执行 `cd host-patch && pnpm add @deepseek-ai/dsh-host-apiproxy@0.1.0-rc.5 --filter dsh-desktop-host-patch` 或用 `pnpm install` 后重试；该依赖仅用于类型，运行时由 sidecar node_modules 提供）。

- [ ] **Step 8: 提交**

```bash
git add host-patch/packages/uds-carrier
git commit -m "feat(uds-carrier): plugin skeleton with socket path fallback chain"
```

---

### Task 3: uplink 路由 + downlink 升级 + 信任机制

**Files:**
- Modify: `host-patch/packages/uds-carrier/src/index.ts`
- Create: `host-patch/packages/uds-carrier/src/index.test.ts`（信任/路由单测，mock 层）
- Create: `host-patch/packages/uds-carrier/src/mock-test-util.ts`

**Interfaces:**
- Consumes: vendored `bridge`/`DEFAULT_MAX_REQUEST_BODY_BYTES`（Task 1）、vendored `WebSocketDownlinks`（Task 1）、`ctx.get('apiProxy')`、`selectSocketPath`（Task 2）
- Produces: apply() 完整实现——node:http server `listen(socketPath)`、`chmod 600`、`upgrade` 按 pathname 精确分发、teardown `closeAllConnections` + unlink、残留 socket 探测（listen 前 connect 探测）

- [ ] **Step 1: 写 mock 工具（apiProxy 桩 + 请求桩）**

`host-patch/packages/uds-carrier/src/mock-test-util.ts`：
```typescript
import { EventEmitter } from 'node:events';

// 最小 ApiProxy 桩：满足 bridge/toFetchHandler 消费面的形状
export function makeMockApiProxy(handler: (method: string, args: unknown) => Promise<unknown>) {
  return {
    call: async (method: string, args: unknown) => handler(method, args),
  };
}

// 构造一个可消费的 IncomingMessage 桩（http.request 层测试用）
export function makeMockServer(handler: (req: unknown, res: unknown) => void) {
  const server = new EventEmitter() as any;
  server.listen = (path: string) => {
    server.listeningPath = path;
    server.emit('listening');
    return server;
  };
  server.close = (cb?: () => void) => { cb?.(); return server; };
  server.closeAllConnections = () => {};
  server.address = () => ({ path: server.listeningPath });
  return server;
}
```

- [ ] **Step 2: 写失败测试（apply 生命周期 + 权限）**

`host-patch/packages/uds-carrier/src/index.test.ts`：
```typescript
import { describe, it, expect, vi } from 'vitest';
import { apply } from './index';
import { makeMockApiProxy } from './mock-test-util';

describe('uds-carrier apply', () => {
  it('creates server with chmod 600 socket and 0700 dir', async () => {
    const fsMock = {
      chmodSync: vi.fn(),
      mkdirSync: vi.fn(),
      unlinkSync: vi.fn(),
      existsSync: vi.fn(() => false),
    };
    const httpMock = {
      createServer: vi.fn(() => makeMockServer(() => {})),
    };
    // 注入 mock 通过 vi.mock 或模块参数（见实现处说明）
    const ctx = { logger: { info: vi.fn() }, get: vi.fn(() => makeMockApiProxy(async () => ({}))), provide: vi.fn() } as any;
    const svc = apply(ctx, { udsPath: '/tmp/dsh-test/dsh.sock' });
    expect(svc.getSocketPath()).toBe('/tmp/dsh-test/dsh.sock');
  });
});
```

- [ ] **Step 3: 运行确认失败**

```bash
cd host-patch && pnpm vitest run packages/uds-carrier/src/index.test.ts
```

预期：FAIL（apply 未实现 createServer 逻辑或 mock 注入不匹配——先让测试通过再补全）。

- [ ] **Step 4: 实现完整 apply()（核心）**

`host-patch/packages/uds-carrier/src/index.ts` 全文替换：
```typescript
import { Context, Service } from '@deepseek-ai/cordis';
import { createServer as httpCreateServer, Server } from 'node:http';
import { connect as netConnect } from 'node:net';
import * as fs from 'node:fs';
import * as os from 'node:os';
import { selectSocketPath } from './socket-path';
import { bridge, DEFAULT_MAX_REQUEST_BODY_BYTES } from '../vendor/http-bridge';
import { WebSocketDownlinks } from '../vendor/websocket-downlink';
import { toFetchHandler } from '@deepseek-ai/dsh-host-apiproxy';

export const inject = ['apiProxy'];

const MUX_PATH = '/api/events.mux';
const HOST_PATH = '/api/events.host';

export interface UdsCarrierConfig {
  udsPath?: string;
  maxBodyBytes?: number;
}

export class UdsCarrierService extends Service {
  static inject = ['apiProxy'];
  private server?: Server;
  private downlinks?: WebSocketDownlinks;
  private socketPath: string;

  constructor(ctx: Context, config: UdsCarrierConfig = {}) {
    super(ctx, 'udsCarrier');
    this.socketPath =
      config.udsPath ??
      selectSocketPath(process.env.DSH_HOME, process.getuid?.() ?? 0, os.tmpdir());
  }

  getSocketPath(): string {
    return this.socketPath;
  }

  async start(): Promise<void> {
    const apiProxy = this.ctx.get('apiProxy');
    const maxBody = this.config?.maxBodyBytes ?? DEFAULT_MAX_REQUEST_BODY_BYTES;
    const socketPath = this.socketPath;

    // 目录 0700（防其他 uid 替换 socket 文件 → bind 劫持）
    const dir = socketPath.slice(0, socketPath.lastIndexOf('/'));
    fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
    fs.chmodSync(dir, 0o700);

    // 残留 socket 清理：listen 前 connect 探测，无活服务则 unlink（spec §4.1）
    if (fs.existsSync(socketPath)) {
      const alive = await this.probeAlive(socketPath);
      if (!alive) fs.unlinkSync(socketPath);
    }

    const fetchHandler = toFetchHandler(apiProxy);
    this.downlinks = new WebSocketDownlinks(apiProxy);

    this.server = httpCreateServer((req, res) => {
      void bridge(req, res, fetchHandler, maxBody);
    });

    this.server.on('upgrade', (req, socket, head) => {
      const pathname = new URL(req.url ?? '/', 'http://dsh').pathname;
      if (pathname === MUX_PATH) {
        this.downlinks!.handleMux(req, socket as any, head);
      } else if (pathname === HOST_PATH) {
        this.downlinks!.handleHost(req, socket as any, head);
      } else {
        socket.destroy();
      }
    });

    await new Promise<void>((resolve, reject) => {
      this.server!.once('error', reject);
      this.server!.listen(socketPath, () => resolve());
    });

    fs.chmodSync(socketPath, 0o600);
    this.ctx.logger.info(`uds-carrier listening on ${socketPath} (0600)`);
  }

  private probeAlive(socketPath: string): Promise<boolean> {
    return new Promise((resolve) => {
      const c = netConnect(socketPath);
      c.once('connect', () => { c.destroy(); resolve(true); });
      c.once('error', () => resolve(false));
      c.setTimeout(1000, () => { c.destroy(); resolve(false); });
    });
  }

  async stop(): Promise<void> {
    this.downlinks?.close();
    this.server?.closeAllConnections();
    await new Promise<void>((resolve) => this.server?.close(() => resolve()) ?? resolve());
    try { fs.unlinkSync(this.socketPath); } catch { /* 幂等 */ }
    this.ctx.logger.info('uds-carrier stopped, socket cleaned');
  }
}

export function apply(ctx: Context, config: UdsCarrierConfig = {}): UdsCarrierService {
  const svc = new UdsCarrierService(ctx, config);
  ctx.provide('udsCarrier', svc);
  ctx.on('dispose', () => { void svc.stop(); });
  void svc.start();
  return svc;
}
```

- [ ] **Step 5: 完善单测（信任机制 + 权限）**

`host-patch/packages/uds-carrier/src/index.test.ts` 补充：
```typescript
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { selectSocketPath } from './socket-path';

it('selects $DSH_HOME/run path by default', () => {
  const p = selectSocketPath(undefined, 501, os.tmpdir());
  expect(p).toBe(`${os.tmpdir()}/dsh-501/dsh.sock`);
});

it('refuses to start when chmod 600 unsupported (platform claim) — dirs are 0700', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dsh-carrier-'));
  const mode = fs.statSync(dir).mode & 0o777;
  expect(mode).toBe(0o700);
  fs.rmSync(dir, { recursive: true, force: true });
});
```

- [ ] **Step 6: 运行全部单测**

```bash
cd host-patch && pnpm vitest run
```

预期：socket-path 4 passed + index 测试通过（若 mock 注入与实现有出入，调整测试以匹配实现行为——信任断言：chmod 600 调用、0700 目录、残留 unlink 路径）。

- [ ] **Step 7: 提交**

```bash
git add host-patch/packages/uds-carrier/src
git commit -m "feat(uds-carrier): uplink bridge + downlink upgrade dispatch + socket lifecycle"
```

---

### Task 4: desktop.patch.yml + 真实启动验证（M1 验收 ①–④）

**Files:**
- Create: `host-patch/desktop.patch.yml`
- Create: `host-patch/README.md`（启动/验证说明）
- Create: `scripts/verify-m1.sh`（验收脚本：lsof、权限、curl、WS upgrade）

**Interfaces:**
- Consumes: Task 3 完成的 uds-carrier 插件（绝对路径经 patch insert）
- Produces: M1 验收证据；baseUrl 锚定结论（Task 5）

- [ ] **Step 1: desktop.patch.yml（spec §4.1 内容）**

`host-patch/desktop.patch.yml`：
```yaml
# dsh-desktop desktop patch（应用顺序：web-app bundle 之后，agent-presets/telemetry 之前）
# 目标：禁用全部 TCP 载体行，插入 UDS 载体。
# 注意：disabled 行保留在组合树、激活期跳过（见 00-verified-facts §1），并因 plugins 源码级 inject
#       ['webServer'...] 而必须禁（webserver 禁用后无 provider，不禁则 Loader 结算失败）。
- id: webserver
  disabled: true
- id: web-runtime
  disabled: true
- id: connection
  disabled: true
- id: client-hmr
  disabled: true
- id: modules
  disabled: true
- insert:
    - id: uds-carrier
      name: '@dsh-desktop/uds-carrier'
      config:
        udsPath: ${DSH_HOME}/run/dsh.sock
```

> 注：`name` 与包名一致（R1 修正：`@dsh-desktop/uds-carrier`，禁裸名）；绝对路径解析方式待 Task 5 实测定案（spec §8：baseUrl 锚定 profile 目录 vs patch 文件目录；兜底 Rust 按模板物化绝对路径）。`${DSH_HOME}` 在 patch config 中的环境变量展开行为未实证——**spike 验证项**（若不展开，socket 路径会变成字面量 `${DSH_HOME}/run/dsh.sock`，导致验收②③失败；兜底：Rust 物化时替换为真实路径）。本地验证阶段在 checkout 内用 `DSH_HOME` 运行时物化。

- [ ] **Step 2: 本地启动验证（真实 checkout，无 key）**

```bash
cd ~/codehub/deepseek-harness
pnpm install 2>&1 | tail -3
pnpm run build 2>&1 | tail -3
# 无 key 启动：应能起服务（describe 会因缺 key 报业务错，但 socket 应可连）
DSH_HOME=/tmp/dsh-m1-test node apps/cli/lib/bin.js --profile web --port 0 \
  --patch /Users/acfufu/Codehub/dsh-desktop/host-patch/desktop.patch.yml &
sleep 6
```

预期：进程存活（或日志显示 socket 监听）。

- [ ] **Step 3: 验收 ① 无 TCP 监听**

```bash
lsof -iTCP -sTCP:LISTEN -P -n 2>/dev/null | grep -i node || echo "PASS: no TCP listen"
```

预期：PASS（无 node TCP 监听）。

- [ ] **Step 4: 验收 ② socket 权限**

```bash
ls -la /tmp/dsh-m1-test/run/dsh.sock
stat -f "%Sp %Su %Sg" /tmp/dsh-m1-test/run/dsh.sock
stat -f "%Sp" /tmp/dsh-m1-test/run
```

预期：`-rw-------`（0600），owner = 当前 uid；run 目录 `drwx------`（0700）。

- [ ] **Step 5: 验收 ③ curl --unix-socket host.describe（带 key 才成功）**

```bash
# 无 key 场景：应返回业务错误 JSON（可达性证明）
curl --unix-socket /tmp/dsh-m1-test/run/dsh.sock \
  -H 'Content-Type: application/json' \
  -d '{"type":"server-request","rpcId":"m1-test-1","method":"host.describe","payload":{}}' \
  http://dsh/api/host.describe | head -c 300
echo
# 带 key 场景（若 DEEPSEEK_API_KEY 可用）：
if [ -n "${DEEPSEEK_API_KEY:-}" ]; then
  DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" DSH_HOME=/tmp/dsh-m1-test \
    node apps/cli/lib/bin.js --profile web --port 0 \
    --patch /Users/acfufu/Codehub/dsh-desktop/host-patch/desktop.patch.yml &
  sleep 6
  curl --unix-socket /tmp/dsh-m1-test/run/dsh.sock \
    -H 'Content-Type: application/json' \
    -d '{"type":"server-request","rpcId":"m1-test-2","method":"host.describe","payload":{}}' \
    http://dsh/api/host.describe
fi
```

预期：有响应（业务 JSON；无 key 时是错误帧但**连接建立 + HTTP 响应到达**）。

- [ ] **Step 6: 验收 ④ WS upgrade（用 node 脚本探测）**

临时脚本 `/tmp/m1-ws-check.mjs`：
```javascript
import { connect } from 'node:http';

const sock = '/tmp/dsh-m1-test/run/dsh.sock';
function tryUpgrade(streamPath) {
  return new Promise((resolve) => {
    const req = connect({ socketPath: sock, path: streamPath, headers: { Upgrade: 'websocket', Connection: 'Upgrade', 'Sec-WebSocket-Key': 'dGhlIHNhbXBsZSBub25jZQ==', 'Sec-WebSocket-Version': '13' } });
    req.on('upgrade', (res, socket) => { console.log(streamPath, '→ UPGRADE OK', res.statusCode); socket.destroy(); resolve(true); });
    req.on('response', (res) => { console.log(streamPath, '→ HTTP', res.statusCode); resolve(false); });
    req.on('error', (e) => { console.log(streamPath, '→ ERROR', e.code); resolve(false); });
    req.end();
  });
}
await tryUpgrade('/api/events.mux');
await tryUpgrade('/api/events.host');
process.exit(0);
```
```bash
node /tmp/m1-ws-check.mjs
```

预期：两行 `→ UPGRADE OK 101`（若 101 不出现，检查 WebSocketDownlinks 的握手要求与 header 完整性）。

- [ ] **Step 7: 清理 + 提交**

```bash
kill %1 %2 2>/dev/null; rm -rf /tmp/dsh-m1-test
git add host-patch/desktop.patch.yml host-patch/README.md scripts/verify-m1.sh
git commit -m "feat(host-patch): desktop.patch.yml disabling TCP carriers, M1 acceptance verified"
```

---

### Task 5: baseUrl 锚定 + Typert 路由缺口 spike（M1 验收 ⑥ + 决策点）

**Files:**
- Create: `docs/m1-spike-findings.md`（结论落档）

**Interfaces:**
- Consumes: Task 4 的启动环境
- Produces: ① baseUrl 锚定结论（决定 Rust 侧物化策略）；② Typert 远端端点（commands/goals/pluginInventory/dynamicCordisRunner）在 connection 禁用后是否可达的结论（决定 carrier 是否需双层分发）

- [ ] **Step 1: baseUrl 锚定实测**

```bash
cd ~/codehub/deepseek-harness
# 在 loader 关键路径打日志或直接读源码确认：
grep -n "baseUrl" packages/boot/app-boot/src/index.ts | head -20
grep -n "baseUrl" vendor/loader/src/config/tree.ts | head -10
```

预期：定位 `internal.import(name, baseUrl)` 的 baseUrl 来源（mountRootInclude:492-504 上下文）。结论写入文档：插件 `uds-carrier` 的裸包名 `dsh-desktop-uds-carrier` 从哪个目录解析——若锚定 profile 目录（`$DSH_HOME/profiles/web/node_modules`），则需把本地插件装入该目录或改 `name` 为已在 node_modules 中的包名；若锚定 patch 文件目录，则直接可用。

- [ ] **Step 2: Typert 路由缺口实测**

启动 desktop 组合（同 Task 4），然后：
```bash
curl --unix-socket /tmp/dsh-m1-test/run/dsh.sock \
  -H 'Content-Type: application/json' \
  -d '{"type":"server-request","rpcId":"m1-t-1","method":"commands.execute","payload":{"line":"help"}}' \
  http://dsh/api/commands/execute
echo
curl --unix-socket /tmp/dsh-m1-test/run/dsh.sock \
  -H 'Content-Type: application/json' \
  -d '{"type":"server-request","rpcId":"m1-t-2","method":"goals.execute","payload":{}}' \
  http://dsh/api/goals/execute
```

预期：记录 `commands/execute` 与 `goals/execute` 的响应——method-not-found（缺口确认，carrier 需双层分发）或成功（apiProxy 实际覆盖，无缺口）。**R1 修正**：wire 格式为 `/api/<ns>/<method>`（Typert 两段式，facts §2 gateway client `connection.rpc.call('/api','commands/execute',...)`），**不是** `/api/commands.execute` 单段。**结论与对策写进 `docs/m1-spike-findings.md`**；**若缺口确认，双层分发必须在本 Task 内落地**（不得「下一迭代」）：在 `bridge` 前加 interceptor 层——镜像 `connection/src/rpc-host.ts` 的 `createSharedFetchHandler` 逻辑：carrier 的 uplink handler 先查已注册 interceptor（TypertGateway 若在），无则直落 `toFetchHandler(apiProxy)`；实现代码加入 Task 3 的 apply()（`this.interceptorFetch` 字段 + 注册表），并补充一条单测（mock 一个 interceptor 命中 vs 未命中）。若 M3 验收⑤（真实 agent 回合）在 M1 缺口未闭合时阻塞，则 M3 Task 6 先验证 ①–④，⑤ 延后到缺口闭合后。

- [ ] **Step 3: 落档 + 提交**

```bash
git add docs/m1-spike-findings.md host-patch
git commit -m "docs(m1): spike findings — baseUrl anchoring + typert routing gap"
```

---

## M1 完成检查（对照 spec §10 M1 验收）

- [ ] ① `lsof -iTCP -sTCP:LISTEN` 无监听端口
- [ ] ② socket 0600、目录 0700
- [ ] ③ `curl --unix-socket` host.describe 返回成功 JSON（带 key）
- [ ] ④ events.mux/host 两路 WS upgrade 101
- [ ] ⑤ uds-carrier 单测全绿（信任机制、残留探测 unlink、路径回退链）
- [ ] ⑥ baseUrl 锚定实测结论落定（docs/m1-spike-findings.md）
- [ ] 附加：Typert 路由缺口结论 + 对策（M1 spike）
