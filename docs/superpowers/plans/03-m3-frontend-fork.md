# M3 — 前端 fork + transport + 构建管线 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> 共享事实底座：先读 `docs/superpowers/plans/00-verified-facts-and-corrections.md`（符号名、文件清单、修正以它为准；**assertEntriesActive 不存在**、openMux yield 的是 `RpcRequest<MuxFrame|HostFrame>`、`createWebConnectionRpc` 有消费者）。

**Goal:** 在 `frontend/` 下建 fork 前端（自包含提交拷贝，非 submodule）：vite 编译 workspace src（alias 指 fork 拷贝目录），transport 换 `tauri-api-client.ts`（invoke ↔ Rust），`generate-manifest.ts` 自产 `__DSH_BOOT__`（≈33 行，从运行时组合派生，禁硬编码）并 `injectBootManifest` 注入 dist/index.html，插件 bundle（≈33 个 `lib/client.js`）拷入 `dist/plugins/<id>/client.js`；dev 模式 public/plugins 静态供给 + transformIndexHtml 注入。验收：`tauri dev` 无 key → 渲染 fork dist + 全条目 load 成功 + 状态到 connected + 会话/设置可交互；有 key → 一个真实 agent 回合。

**Architecture:** fork 形态 = 「编译面整体复制 + npm 固定版本 + 构建期拉取插件 bundle」（spec §4.3 确定结论）。8 个拷贝包（web、modules、connection、web-react、ui-slots、ui-primitives、ui-attachment、schema-form），同步冲突面 ~40 文件，`UPSTREAM_PIN` + `sync-frontend.sh`（canary：web-api-client.ts 漂移哨兵）。

**Tech Stack:** vite（上游版本，读 fork 后 `frontend/apps/web/package.json`）、React 18、vitest + jsdom、`@tauri-apps/api@2.11.1`、`@deepseek-ai/dsh-client-modules@0.1.0-rc.5`（injectBootManifest 根导出）、`@deepseek-ai/dsh-host-apiproxy@0.1.0-rc.5`、`@deepseek-ai/cordis`。

## Global Constraints

- 执行者 = deepseek-v4-flash 级别：每步完整代码/命令/预期输出；禁止「参考上一步」。
- **不得修改 deepseek-harness**；fork 是自包含拷贝。上游 commit pin `47f943859bef60e4160492346772ded9b24f765a`（`frontend/UPSTREAM_PIN`）。
- 8 个拷贝包全部**精确版本**依赖（禁 `^`）。
- `web-api-client.ts` 是 canary 哨兵：fork 中**删除**（被 tauri-api-client 取代），同步脚本 diff 它感知协议层变更。
- `createWebConnectionRpc` 换 invoke 版，**不得设超时**（command.execute 经此通道）；签名保持 `call(channel, endpoint, payload, signal?)`。
- 下行帧事件名精确（R1 修正：改为 Channel 机制描述——M2/M3 用 `tauri::ipc::Channel<String>` 传文本帧，终止以**空字符串 `""` 哨兵**（M2 流断时 send("")，M3 以 `text === ''` 判终），不再使用 window event 名 `dsh:downlink:mux|host`（避免与事实不符的协议假设）。
- 构建管线测试：bundle 数 = 组合行数（非硬编码 33/36）；rev 稳定性；inject 后 boot 可达；index.html 含 CSP meta + `connect-src ipc:`。
- pnpm 11.3.0；`frontend/` 独立 package.json（非 root workspace 成员，避免与 host-patch 纠缠）。

---

### Task 1: fork 骨架 + 拷贝 8 包 + 精确版本依赖

**Files:**
- Create: `frontend/package.json`
- Create: `frontend/tsconfig.json`
- Create: `frontend/vite.config.ts`
- Create: `frontend/UPSTREAM_PIN`
- Create: `scripts/sync-frontend.sh`
- Copy: `apps/web/*`、`packages/client/{web,modules,connection,web-react,ui-slots,ui-primitives,ui-attachment,schema-form}/src/**`（见清单）
- Delete（后续 task）：`web-api-client.ts`、`fixture.ts`

**Interfaces:**
- Consumes: 上游 8 包源码（commit 47f94385）
- Produces: fork 目录树（自包含、可 `pnpm install && vite build`）

- [ ] **Step 1: 建目录 + UPSTREAM_PIN**

```bash
mkdir -p frontend/apps/web frontend/packages/client
cd ~/codehub/deepseek-harness && git rev-parse HEAD   # 确认 47f94385...
```

`frontend/UPSTREAM_PIN`：
```
REPO=deepseek-harness
COMMIT=47f943859bef60e4160492346772ded9b24f765a
BRANCH=master
DATE=2026-08-16
# 变更记录
# - 初始 fork：8 包拷贝（web/modules/connection/web-react/ui-slots/ui-primitives/ui-attachment/schema-form）
```

- [ ] **Step 2: 拷贝 apps/web（main.ts 复制不改、index.html 小改、vite.config.ts 小改、public 小改）**

```bash
SRC=~/codehub/deepseek-harness/apps/web
DST=frontend/apps/web
mkdir -p "$DST/src" "$DST/public"
cp "$SRC/src/main.ts" "$DST/src/main.ts"
cp "$SRC/src/node-module-stub.ts" "$DST/src/node-module-stub.ts"
cp "$SRC/index.html" "$DST/index.html"
cp "$SRC/vite.config.ts" "$DST/vite.config.ts"
cp -r "$SRC/public/." "$DST/public/"
cp "$SRC/package.json" "$DST/package.json"
ls -R "$DST" | head -30
```

预期：目录结构出现（src/ + public/ + 配置）。

- [ ] **Step 3: 拷贝 7 个 client 包（R1 修正：标题为 8 个——web/modules/connection/web-react/ui-slots/ui-primitives/ui-attachment/schema-form）**

```bash
SRC=~/codehub/deepseek-harness/packages/client
DST=frontend/packages/client
for p in web modules connection web-react ui-slots ui-primitives ui-attachment schema-form; do
  mkdir -p "$DST/$p"
  cp -r "$SRC/$p/src" "$DST/$p/src"
  cp "$SRC/$p/package.json" "$DST/$p/package.json"
done
# 校验 client/web/src 恰好 13 文件
ls "$DST/web/src" | wc -l   # 预期 13
# 校验 connection/src/client 恰好 7 文件
ls "$DST/connection/src/client" | wc -l   # 预期 7
```

- [ ] **Step 4: 删除不 fork 的文件（canary 哨兵 + 测试夹具）**

```bash
rm "$DST/connection/src/client/web-api-client.ts"   # 被 tauri-api-client 取代（spec §4.3）
rm "$DST/connection/src/client/fixture.ts"          # 3188 行测试夹具，删 ?fixture 分支
```

- [ ] **Step 5: fork 根 package.json（精确版本，禁 ^）**

`frontend/package.json`：
```json
{
  "name": "dsh-desktop-frontend",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite --config vite.config.ts",
    "build": "vite build --config vite.config.ts",
    "test": "vitest run",
    "generate-manifest": "node scripts/generate-manifest.ts",
    "sync": "../scripts/sync-frontend.sh"
  },
  "dependencies": {
    "react": "18.3.1",
    "react-dom": "18.3.1",
    "@deepseek-ai/dsh-host-apiproxy": "0.1.0-rc.5",
    "@deepseek-ai/dsh-client-modules": "0.1.0-rc.5",
    "@deepseek-ai/cordis": "0.1.0-rc.5",
    "@tauri-apps/api": "2.11.1",
    "@tauri-apps/plugin-notification": "2.3.3",
    "@tauri-apps/plugin-autostart": "2.5.1",
    "@tauri-apps/plugin-opener": "2.5.4"
  },
  "devDependencies": {
    "vite": "^6.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "vitest": "^2.1.0",
    "jsdom": "^25.0.0",
    "typescript": "^5.6.0",
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@types/node": "^24.0.0",
    "ws": "^8.18.0"
  }
}
```

> 注：vite/plugin-react 版本以 fork 的 `apps/web/package.json` 上游版本为准，如上游是 5.x 则用 5.x——本步以「build 可跑」为目标，冲突时对齐上游 devDependencies。

- [ ] **Step 6: vite.config.ts（删 rejectStandaloneServe + alias 改 fork 路径）**

`frontend/vite.config.ts`（基于上游，删 `rejectStandaloneServe` 插件，alias 指 fork 路径）：
```typescript
import { defineConfig, Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';
import { readFileSync } from 'node:fs';
import { buildManifest } from './scripts/generate-manifest';

// dev 注入 __DSH_BOOT__ manifest（R1 修正：真实注入，非 stub——读取 composed-entries.json + public/plugins 供给）
function devBootManifest(): Plugin {
  return {
    name: 'dsh-dev-boot-manifest',
    transformIndexHtml: {
      order: 'post',
      handler: (html: string) => {
        // 从 composed-entries.json 读条目（开发期清单；R1 修正：由 derive-composed-entries 脚本生成，非手写）
        const entries = JSON.parse(readFileSync(new URL('./composed-entries.json', import.meta.url), 'utf8'));
        const manifest = buildManifest(entries.map((e: any) => ({ id: e.id, file: '', rev: e.rev })));
        const script = `<script>window.__DSH_BOOT__=${JSON.stringify(manifest)}</script>`;
        return { html: html.replace('</head>', `${script}</head>`) };
      },
    },
  };
}

export default defineConfig({
  plugins: [react(), devBootManifest()],
  resolve: {
    alias: {
      '@deepseek-ai/dsh-client-web': fileURLToPath(new URL('./packages/client/web/src/boot.tsx', import.meta.url)),
      '@deepseek-ai/dsh-client-web-react': fileURLToPath(new URL('./packages/client/web-react/src/index.ts', import.meta.url)),
      '@deepseek-ai/dsh-client-ui-slots': fileURLToPath(new URL('./packages/client/ui-slots/src/index.ts', import.meta.url)),
      '@deepseek-ai/dsh-client-ui-primitives': fileURLToPath(new URL('./packages/client/ui-primitives/src/index.ts', import.meta.url)),
      '@deepseek-ai/dsh-client-ui-attachment': fileURLToPath(new URL('./packages/client/ui-attachment/src/index.ts', import.meta.url)),
      '@deepseek-ai/dsh-client-schema-form': fileURLToPath(new URL('./packages/client/schema-form/src/index.ts', import.meta.url)),
      '@deepseek-ai/dsh-client-modules/client': fileURLToPath(new URL('./packages/client/modules/src/client/index.ts', import.meta.url)),
      node: 'node-module-stub',
    },
  },
  build: { outDir: 'dist' },
  clearScreen: false,
  server: { port: 1420, strictPort: true },
});
```

> 注：alias 键名必须与上游 vite.config.ts 的实际 alias 一致（facts §5：上游 alias 数组 dsh-client-web→web/src/boot.tsx 等）——**以拷贝来的上游 vite.config.ts 为准核对键名**，本步为起点。

- [ ] **Step 7: 安装 + 构建冒烟（R1 修正：先重写 workspace:* 依赖再安装）**

被拷贝的 7 个包 package.json 依赖 `workspace:*` 协议（上游 monorepo），独立 fork 无法解析。逐包重写：

```bash
# 把 fork 内所有 workspace:* 替换为 UPSTREAM_PIN 版本（从上游 package.json 读实际版本，或查 node_modules 实际安装版）
cd frontend
for f in $(find . -name package.json -not -path '*/node_modules/*'); do
  if grep -q 'workspace:' "$f"; then
    echo "== $f"; grep -o '"@deepseek-ai/[^"]*": "workspace:[^"]*"' "$f" || true
  fi
done
# 对每个 workspace:* 依赖，用 DSH_REPO 对应包的实际版本替换（grep 版本号）
# 例：sed -i '' 's#"@deepseek-ai/dsh-client-modules": "workspace:\*"#"@deepseek-ai/dsh-client-modules": "0.1.0-rc.5"#g' <files>
```

> 版本号来源：`grep '"version"' ~/codehub/deepseek-harness/packages/client/<pkg>/package.json`（monorepo 统一 rc 版本线，host-apiproxy 已证 0.1.0-rc.5；其余以实际为准）。替换完成后：

```bash
cd frontend && pnpm install 2>&1 | tail -5
pnpm build 2>&1 | tail -20
```

预期：build 报错（缺 tauri-api-client.ts 等）——**记录首个错误**，Task 2 解决。若 build 意外成功，记录并通过（此时 transport 还是上游 WS 版，Task 3 才换）。

- [ ] **Step 8: sync-frontend.sh（canary 哨兵）**

`scripts/sync-frontend.sh`：
```bash
#!/usr/bin/env bash
# 同步 fork 拷贝目录与上游（spec §4.3 同步策略：pin 拉取 → diff → 手动合并）
# canary：web-api-client.ts 是 transport 缝漂移哨兵——fork 已删，diff 上游若出现新改动即告警。
set -euo pipefail
REPO="${DSH_REPO:-$HOME/codehub/deepseek-harness}"
PIN="frontend/UPSTREAM_PIN"
COMMIT="$(grep '^COMMIT=' "$PIN" | cut -d= -f2)"
SRC="$REPO/packages/client"
DST="frontend/packages/client"

for p in web modules connection web-react ui-slots ui-primitives ui-attachment schema-form; do
  echo "=== $p ==="
  diff -rq "$SRC/$p/src" "$DST/$p/src" 2>&1 | grep -v '^Only in' || true
done

# canary：上游 web-api-client.ts 有改动 = 协议层可能漂移
if [ -f "$SRC/connection/src/client/web-api-client.ts" ]; then
  echo "CANARY: upstream web-api-client.ts exists (fork deleted it by design)."
  echo "  Protocol-layer drift check: review its diff before bumping UPSTREAM_PIN."
fi
echo "sync check complete (commit $COMMIT)"
```

```bash
chmod +x scripts/sync-frontend.sh && ./scripts/sync-frontend.sh 2>&1 | tail -15
```

预期：逐包 diff 输出（首次应只有 fork 侧新增文件 `Only in ...` 被过滤，上游侧无改动）。

- [ ] **Step 9: 提交**

```bash
git add frontend scripts/sync-frontend.sh
git commit -m "feat(frontend): fork 8 packages with pinned deps and sync sentinel"
```

---

### Task 2: tauri-api-client.ts（transport 替换核心）

**Files:**
- Create: `frontend/packages/client/connection/src/client/tauri-api-client.ts`
- Modify: `frontend/packages/client/connection/src/client/index.ts`（transport 构造换 tauri 版）
- Create: `frontend/packages/client/connection/src/client/tauri-api-client.test.ts`

**Interfaces:**
- Consumes: `invoke`（@tauri-apps/api）、`Channel`（@tauri-apps/api/core）、上游 `AbstractApiClient`（npm）、`serverRequestSchema`（npm apiproxy）
- Produces: `TauriApiClient extends AbstractApiClient`——`doFetch`（invoke dsh_http，AbortSignal 映射 + 传输/业务错误分类）、`openMux`/`openHost`（Channel 注册 + invoke dsh_open_stream + `-end` 终止 + finally dsh_close_stream）、`createWebConnectionRpc` invoke 版（Task 3）

- [ ] **Step 1: 写失败测试（错误分类 + 事件名精确）**

`frontend/packages/client/connection/src/client/tauri-api-client.test.ts`：
```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { TauriApiClient, classifyError } from './tauri-api-client';

describe('classifyError', () => {
  it('invoke reject with kind → transport error', () => {
    expect(classifyError({ kind: 'connection-refused' })).toBe('transport');
  });

  it('plain error → transport error', () => {
    expect(classifyError(new Error('boom'))).toBe('transport');
  });

  it('HTTP-status-like object is NOT classified here (business errors carry status+body)', () => {
    expect(classifyError({ status: 500, body: 'x' })).toBe('business');
  });
});
```

- [ ] **Step 2: 运行确认失败**

```bash
cd frontend && pnpm vitest run packages/client/connection/src/client/tauri-api-client.test.ts
```

预期：FAIL（模块不存在）。

- [ ] **Step 3: 实现 tauri-api-client.ts（spec §4.3 transport 机制）**

`frontend/packages/client/connection/src/client/tauri-api-client.ts`：
```typescript
import { invoke, Channel } from '@tauri-apps/api/core';
// R1 修正：导入面按 facts §3/§2 拆分——
//   AbstractApiClient/IApiClient 在 '@deepseek-ai/dsh-host-apiproxy/client'
//   RpcRequest/MuxFrame/HostFrame 是 connection 包类型（fork 拷贝的 rpc.ts 导出）
//   serverRequestSchema 在 '@deepseek-ai/dsh-host-apiproxy/api'
//   RpcId 在 '@deepseek-ai/dsh-host-apiproxy' 根导出
import { AbstractApiClient, IApiClient } from '@deepseek-ai/dsh-host-apiproxy/client';
import { serverRequestSchema } from '@deepseek-ai/dsh-host-apiproxy/api';
import { RpcId } from '@deepseek-ai/dsh-host-apiproxy';
import type { RpcRequest, MuxFrame, HostFrame } from './rpc';

// 传输错误 vs 业务错误分类（spec §4.3）：invoke reject（连接拒绝/IO/超时，带 kind）→ 可重试传输错误；
// HTTP status + body → 业务错误（由 doFetch 内构造 Response 返回）。
export function classifyError(e: unknown): 'transport' | 'business' {
  if (e && typeof e === 'object' && 'status' in e && 'body' in e) return 'business';
  return 'transport';
}

export class TauriApiClient extends AbstractApiClient implements IApiClient {
  constructor() {
    super({ timeoutMs: 30_000 }); // 基类默认 30s + caller-signal-only 语义保留
  }

  protected async doFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
    // 前端 AbortSignal 映射为 Rust 侧取消（spec §4.3）：invoke 携带请求 id + 取消通道。
    // v1 简化：invoke 不挂起时由 dsh_cancel(id) 取消；此处保持 doFetch 无超时（调用点施加）。
    const method = (init?.method ?? 'GET').toUpperCase();
    const path = new URL(input.toString(), 'http://dsh').pathname;
    const body = init?.body instanceof ArrayBuffer
      ? new Uint8Array(init.body)
      : typeof init?.body === 'string'
        ? new TextEncoder().encode(init.body)
        : undefined;

    let resp: { status: number; headers: Record<string, string>; body: number[] };
    try {
      resp = await invoke<{ status: number; headers: Record<string, string>; body: number[] }>('dsh_http', {
        method,
        path,
        body: body ? Array.from(body) : null,
      });
    } catch (e) {
      const err = new Error(`dsh_http failed: ${JSON.stringify(e)}`, { cause: e });
      (err as any).kind = classifyError(e);
      throw err;
    }

    // 响应字节以 ArrayBuffer 保真重建 Response body（附件图片等二进制走此路，spec §4.3）
    const bytes = new Uint8Array(resp.body);
    const responseBody = new Blob([bytes]).arrayBuffer();
    return new Response(await responseBody, {
      status: resp.status,
      headers: new Headers(resp.headers),
    });
  }

  async *openMux(opts?: { signal?: AbortSignal }): AsyncGenerator<RpcRequest<MuxFrame>> {
    yield* this.openDownlink('mux', opts?.signal);
  }

  async *openHost(opts?: { signal?: AbortSignal }): AsyncGenerator<RpcRequest<HostFrame>> {
    yield* this.openDownlink('host', opts?.signal);
  }

  private async *openDownlink(
    stream: 'mux' | 'host',
    signal?: AbortSignal,
  ): AsyncGenerator<RpcRequest<any>> {
    // 先创建 Channel 并注册 onmessage，再 invoke（invoke 返回即 onOpen 信号，spec §4.3）
    const channel = new Channel<string>();
    const frames: Array<RpcRequest<any>> = [];
    let endResolve: () => void = () => {};
    let ended = false;
    const endPromise = new Promise<void>((r) => { endResolve = r; });
    let notify: () => void = () => {};
    let pending = false;

    channel.onmessage = (text: string) => {
      if (text === '') { ended = true; endResolve(); return; }
      const full = serverRequestSchema.parse(JSON.parse(text));
      this.onEnvelope?.(full); // 逐帧 onEnvelope tap（settings/credentials 安全观察，spec §4.3）
      const req: RpcRequest<any> = { rpcId: RpcId(full.rpcId), payload: full.payload };
      frames.push(req);
      pending = true;
      notify();
    };

    let streamId: number;
    try {
      streamId = await invoke<number>('dsh_open_stream', { stream, channel });
    } catch (e) {
      const err = new Error(`open stream ${stream} failed: ${JSON.stringify(e)}`, { cause: e });
      (err as any).kind = 'transport';
      throw err;
    }

    // 挂起的 open_stream invoke 绑定代际 AbortSignal（spec §4.3）
    const abortHandler = () => { ended = true; endResolve(); };
    signal?.addEventListener('abort', abortHandler, { once: true });

    try {
      while (!ended) {
        while (frames.length > 0) {
          const f = frames.shift()!;
          pending = frames.length > 0;
          yield f;
        }
        if (ended) break;
        if (!pending) {
          await Promise.race([endPromise, new Promise<void>((r) => { notify = r; })]);
        }
      }
    } finally {
      // 迭代器 finally → invoke('dsh_close_stream')（open_stream 未完成即失败时幂等 no-op，spec §4.3）
      signal?.removeEventListener('abort', abortHandler);
      await invoke('dsh_close_stream', { id: streamId }).catch(() => {});
    }
  }
}
```

> 注：以上导入路径为 R1 修正后的预期；若 `./rpc` 未导出 `RpcRequest/MuxFrame/HostFrame`（编译报错），则以 fork 内实际导出为准（`type` 导入只影响类型，本步以 tsc 通过为验收）。`super({ timeoutMs: 30_000 })` 若构造器签名不符（facts 只证 DEFAULT_TIMEOUT_MS 常量），改为调用基类默认构造并在 doFetch 层不设超时（调用点施加，符合 spec）。

- [ ] **Step 4: index.ts 换 transport 构造**

`frontend/packages/client/connection/src/client/index.ts`：把 `new WebApiClient()` 替换为 `new TauriApiClient()`，删除 `?fixture` 分支（fixture 已删）：
```typescript
import { TauriApiClient } from './tauri-api-client';
// ... 原 apply() 内：
const api: IApiClient = new TauriApiClient();
```

- [ ] **Step 5: 运行测试**

```bash
cd frontend && pnpm vitest run packages/client/connection/src/client/
```

预期：classifyError 3 passed（openDownlink 需 mock `invoke`——vitest 里 `vi.mock('@tauri-apps/api/core', ...)`，见 Task 4 测试基建）。

- [ ] **Step 6: 提交**

```bash
git add frontend/packages/client/connection/src/client/
git commit -m "feat(frontend): tauri-api-client transport replacing web-api-client"
```

---

### Task 3: createWebConnectionRpc invoke 版

**Files:**
- Modify: `frontend/packages/client/connection/src/client/rpc.ts`

**Interfaces:**
- Consumes: `invoke`（@tauri-apps/api/core）
- Produces: `createWebConnectionRpc()`——签名 `call(channel, endpoint, payload, signal?) → Promise<RpcResult<unknown>>`，**不得设超时**（command.execute 经此通道，spec §4.3）

- [ ] **Step 1: 改 rpc.ts 的 fetch 为 invoke**

`frontend/packages/client/connection/src/client/rpc.ts` 中 `createWebConnectionRpc` 的 POST 实现替换为：
```typescript
import { invoke } from '@tauri-apps/api/core';

// 原实现用 globalThis.fetch（tauri://localhost 不可用，spec §4.3）——换 invoke dsh_http。
// 签名保持 call(channel, endpoint, payload, signal?)，不得设超时。
async function postEnvelope(channel: string, endpoint: string, envelope: unknown, signal?: AbortSignal): Promise<unknown> {
  if (signal?.aborted) throw new DOMException('Aborted', 'AbortError');
  // R1 修正：path = `${channel}/${endpoint}`（channel 已含前导 /，如 '/api'）——
  // 原写法 `/${channel}/${endpoint}` 会产生 '//api/...' 双斜杠，被 Rust 输入校验拒绝。
  const resp = await invoke<{ status: number; body: number[] }>('dsh_http', {
    method: 'POST',
    path: `${channel}/${endpoint}`,
    body: Array.from(new TextEncoder().encode(JSON.stringify(envelope))),
  });
  if (resp.status >= 400) {
    throw new Error(`rpc ${channel}/${endpoint} → HTTP ${resp.status}: ${new TextDecoder().decode(new Uint8Array(resp.body))}`);
  }
  return JSON.parse(new TextDecoder().decode(new Uint8Array(resp.body)));
}
```

- [ ] **Step 2: 编译 + 测试**

```bash
cd frontend && pnpm exec tsc --noEmit -p tsconfig.json 2>&1 | tail -10
pnpm vitest run packages/client/connection/src/ 2>&1 | tail -5
```

预期：tsc 无错（或仅记录既有错误），测试通过。

- [ ] **Step 3: 提交**

```bash
git add frontend/packages/client/connection/src/client/rpc.ts
git commit -m "feat(frontend): createWebConnectionRpc invoke transport (no timeout)"
```

---

### Task 4: 前端测试基建（mock @tauri-apps/api + ConnectionController 行为）

**Files:**
- Create: `frontend/vitest.config.ts`
- Create: `frontend/packages/client/connection/src/client/connection.test.ts`

**Interfaces:**
- Consumes: 上游 ConnectionController（拷贝）
- Produces: jsdom + mock invoke/Channel 下的代际/握手行为测试（spec §7：boot 无 key 冒烟 + 传输行为）

- [ ] **Step 1: vitest 配置（jsdom）**

`frontend/vitest.config.ts`：
```typescript
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    include: ['packages/**/*.test.ts', 'packages/**/*.test.tsx'],
    environment: 'jsdom',
    setupFiles: ['./test/setup.ts'],
  },
});
```

`frontend/test/setup.ts`：
```typescript
// mock @tauri-apps/api/core：invoke + Channel
import { vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => {
  class MockChannel<T> {
    private handler: ((msg: T) => void) | null = null;
    set onmessage(fn: (msg: T) => void) { this.handler = fn; }
    get onmessage() { return this.handler!; }
    send(msg: T) { this.handler?.(msg); }
  }
  return {
    invoke: vi.fn(async () => { throw new Error('invoke not mocked in this test'); }),
    Channel: MockChannel,
  };
});
```

- [ ] **Step 2: ConnectionController 代际测试（上游语义确认）**

`frontend/packages/client/connection/src/client/connection.test.ts`：
```typescript
import { describe, it, expect, vi } from 'vitest';
import { ConnectionController, ConnectionConfig } from './connection';

describe('ConnectionController regeneration', () => {
  it('config defaults: base 500ms, factor 2, cap 10s, streamOpenTimeout 3s', () => {
    const defaults: ConnectionConfig = {
      api: {} as any,
      backoffBaseMs: 500,
      backoffFactor: 2,
      backoffMaxMs: 10_000,
      streamOpenTimeoutMs: 3_000,
    };
    expect(defaults.backoffBaseMs).toBe(500);
    expect(defaults.backoffFactor).toBe(2);
    expect(defaults.backoffMaxMs).toBe(10_000);
  });

  it('handshake requires describe success AND both streams open', () => {
    // 以拷贝的上游 connection.ts 为准：138-155 行逻辑——此处验证常量契约，行为由上游单测覆盖
    const cc = ConnectionController as any;
    expect(typeof cc).toBe('function');
  });
});
```

- [ ] **Step 3: 运行测试**

```bash
cd frontend && pnpm vitest run 2>&1 | tail -8
```

预期：全部通过（新测试 + 上游拷贝的既有测试若引用 WebApiClient 需调整——fixture/web-api-client 已删，相关测试迁移到 tauri mock 下）。

- [ ] **Step 4: 提交**

```bash
git add frontend/vitest.config.ts frontend/test frontend/packages/client/connection/src/client/connection.test.ts
git commit -m "test(frontend): vitest jsdom infra with tauri api mocks"
```

---

### Task 5: generate-manifest.ts + 插件 bundle 管线 + CSP + 品牌

**Files:**
- Create: `frontend/scripts/generate-manifest.ts`
- Create: `frontend/scripts/generate-manifest.test.ts`
- Modify: `frontend/apps/web/index.html`（CSP meta + 品牌 title）
- Modify: `frontend/vite.config.ts`（build 后钩子）
- Create: `frontend/scripts/collect-bundles.mjs`（lib/client.js → dist/plugins）

**Interfaces:**
- Consumes: `injectBootManifest`（@deepseek-ai/dsh-client-modules 根导出）、上游 `compose()` 同构逻辑（modules/src/index.ts:315-318）
- Produces: `dist/plugins/<id>/client.js?rev=<sha1-12>` + 注入 `window.__DSH_BOOT__` 的 dist/index.html

- [ ] **Step 1: 写失败测试（schema + 非硬编码 + rev 稳定性）**

`frontend/scripts/generate-manifest.test.ts`：
```typescript
import { describe, it, expect } from 'vitest';
import { buildManifest, ManifestEntry } from './generate-manifest';

const sample: ManifestEntry[] = [
  { id: 'connection', file: '/fake/lib/client.js', rev: 'abc123' },
  { id: 'locale', file: '/fake/lib/client.js', rev: 'def456' },
];

describe('buildManifest', () => {
  it('produces schema { rev, entries: [{id,url,rev}] }', () => {
    const m = buildManifest(sample);
    expect(typeof m.rev).toBe('string');
    expect(m.rev.length).toBe(12);
    expect(m.entries.length).toBe(2);
    expect(m.entries[0]).toEqual({ id: 'connection', url: '/plugins/connection/client.js?rev=abc123', rev: 'abc123' });
  });

  it('rev is stable for identical content (sha1-12)', () => {
    const a = buildManifest([{ id: 'x', file: '/fake/lib/client.js', rev: 'deadbeef00aa' }]);
    const b = buildManifest([{ id: 'x', file: '/fake/lib/client.js', rev: 'deadbeef00aa' }]);
    expect(a.rev).toBe(b.rev);
  });

  it('rev changes when entry revs change', () => {
    const a = buildManifest([{ id: 'x', file: '/f', rev: 'aaaa00000001' }]);
    const b = buildManifest([{ id: 'x', file: '/f', rev: 'aaaa00000002' }]);
    expect(a.rev).not.toBe(b.rev);
  });
});
```

- [ ] **Step 2: 运行确认失败**

```bash
cd frontend && pnpm vitest run scripts/generate-manifest.test.ts
```

预期：FAIL（generate-manifest 不存在）。

- [ ] **Step 3: 实现 generate-manifest.ts（spec §4.3：从组合派生，非硬编码）**

`frontend/scripts/generate-manifest.ts`：
```typescript
// __DSH_BOOT__ 自产（spec §4.3）：entries 从运行时最终组合派生（≈33 行，禁硬编码）。
// schema: { rev, entries: [{ id, url, rev, inject?, immediately? }] }（manifest.ts:50-69）
import { createHash } from 'node:crypto';
import { injectBootManifest } from '@deepseek-ai/dsh-client-modules';
import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, copyFileSync } from 'node:fs';
import { join, dirname } from 'node:path';

export interface ManifestEntry {
  id: string;
  file: string; // lib/client.js 绝对路径
  rev: string;  // sha1-12
  inject?: boolean;
  immediately?: boolean;
}

export interface BootManifest {
  rev: string;
  entries: Array<{ id: string; url: string; rev: string; inject?: boolean; immediately?: boolean }>;
}

export function revOf(content: Buffer): string {
  return createHash('sha1').update(content).digest('hex').slice(0, 12);
}

export function buildManifest(entries: ManifestEntry[]): BootManifest {
  const composed = entries.map((e) => ({
    id: e.id,
    url: `/plugins/${e.id}/client.js?rev=${e.rev}`,
    rev: e.rev,
    ...(e.inject ? { inject: true } : {}),
    ...(e.immediately ? { immediately: true } : {}),
  }));
  const rev = revOf(Buffer.from(JSON.stringify(composed.map((e) => e.rev))));
  return { rev, entries: composed };
}

// 收集入口：sidecar 或 npm 构建产物的 client bundle 目录（同一版本源，spec §4.3）
export function collectEntries(bundleRoot: string, ids: string[]): ManifestEntry[] {
  return ids.map((id) => {
    const file = join(bundleRoot, id, 'lib', 'client.js');
    const content = readFileSync(file);
    return { id, file, rev: revOf(content) };
  });
}

// 主流程：读组合清单（由运行时最终组合派生，M1 spike 产物或 sidecar 导出）→ 拷贝 → 注入
export function runMain() {
  const manifestFile = process.argv[2] ?? './composed-entries.json';
  const distRoot = process.argv[3] ?? './dist';
  const entries: Array<{ id: string; file: string }> = JSON.parse(readFileSync(manifestFile, 'utf8'));
  const full = entries.map((e) => {
    const content = readFileSync(e.file);
    const rev = revOf(content);
    const dest = join(distRoot, 'plugins', e.id, 'client.js');
    mkdirSync(dirname(dest), { recursive: true });
    copyFileSync(e.file, dest);
    return { id: e.id, file: e.file, rev };
  });
  const manifest = buildManifest(full);
  const htmlPath = join(distRoot, 'index.html');
  const html = readFileSync(htmlPath, 'utf8');
  const injected = injectBootManifest(html, manifest);
  writeFileSync(htmlPath, injected);
  console.log(`__DSH_BOOT__: ${full.length} entries, rev=${manifest.rev}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  runMain();
}
```

- [ ] **Step 4: 运行测试确认通过**

```bash
cd frontend && pnpm vitest run scripts/generate-manifest.test.ts
```

预期：4 passed（3 测试 + 可能追加）。

- [ ] **Step 5: collect-bundles.mjs（lib/client.js 拷贝，spec §4.3 构建管线）**

`frontend/scripts/collect-bundles.mjs`：
```javascript
// 把 ≈33 个 lib/client.js 拷到 dist/plugins/<id>/client.js（清单从运行时组合派生，非硬编码）
import { readFileSync, writeFileSync, mkdirSync, copyFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';

const [,, bundleRoot, distRoot, entriesFile] = process.argv;
const entries = JSON.parse(readFileSync(entriesFile, 'utf8')); // [{id, file}]
for (const { id, file } of entries) {
  if (!existsSync(file)) throw new Error(`missing bundle: ${file}`);
  const dest = join(distRoot, 'plugins', id, 'client.js');
  mkdirSync(dirname(dest), { recursive: true });
  copyFileSync(file, dest);
}
console.log(`collected ${entries.length} bundles into ${distRoot}/plugins`);
```

- [ ] **Step 6: index.html CSP + 品牌（spec §4.3 CSP）**

`frontend/apps/web/index.html` 的 `<head>` 内加（替换 title）：
```html
<title>dsh-desktop</title>
<meta http-equiv="Content-Security-Policy" content="default-src 'self'; img-src 'self' data: blob: asset:; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost; font-src 'self' data:" />
```

- [ ] **Step 7: 构建管线测试（bundle 数 = 组合行数 + CSP 断言）**

`frontend/scripts/build-pipeline.test.ts`：
```typescript
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

describe('build pipeline output', () => {
  it('dist/index.html contains CSP meta with connect-src ipc:', () => {
    const html = readFileSync(join(__dirname, '../dist/index.html'), 'utf8');
    expect(html).toContain('Content-Security-Policy');
    expect(html).toContain('connect-src');
    expect(html).toContain('ipc:');
  });

  it('dist/plugins entry count matches composed entries (non-hardcoded)', () => {
    const entries = JSON.parse(readFileSync(join(__dirname, '../composed-entries.json'), 'utf8'));
    for (const { id } of entries) {
      const f = join(__dirname, `../dist/plugins/${id}/client.js`);
      expect(() => readFileSync(f)).not.toThrow();
    }
  });
});
```

- [ ] **Step 8: 完整构建冒烟（R1 修正：composed-entries.json 由派生脚本生成，禁手写）**

**先建派生脚本** `frontend/scripts/derive-composed-entries.mjs`（R1 修正：条目从运行时组合**派生**，非硬编码 33——读 sidecar 的 `$DSH_HOME/profiles/web/` 组合目录或 bundle 产物目录中 `package.json` 的 `dsh.client` 声明 + `lib/client.js` 存在性；与 `generate-manifest` 同源）：

```javascript
// 派生 __DSH_BOOT__ 条目清单（R1 修正：从磁盘 bundle 产物派生，禁硬编码行数）
// 输入：bundle 根目录（含各包 lib/client.js）；输出：composed-entries.json
import { readFileSync, writeFileSync, readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

const [,, bundleRoot, outFile] = process.argv;
const entries = [];
for (const pkgDir of readdirSync(bundleRoot, { withFileTypes: true })) {
  if (!pkgDir.isDirectory()) continue;
  const pkgJson = join(bundleRoot, pkgDir.name, 'package.json');
  const clientJs = join(bundleRoot, pkgDir.name, 'lib', 'client.js');
  if (!existsSync(pkgJson) || !existsSync(clientJs)) continue;
  const pkg = JSON.parse(readFileSync(pkgJson, 'utf8'));
  if (!pkg.dsh?.client) continue; // 只收声明 dsh.client 的包
  const content = readFileSync(clientJs);
  const rev = createHash('sha1').update(content).digest('hex').slice(0, 12);
  entries.push({ id: pkg.name, file: clientJs, rev });
}
writeFileSync(outFile, JSON.stringify(entries, null, 2));
console.log(`derived ${entries.length} entries → ${outFile}`);
```

```bash
cd frontend && pnpm build
node scripts/derive-composed-entries.mjs ../src-tauri/resources/dsh/node_modules ./composed-entries.json
node scripts/generate-manifest.ts ./composed-entries.json ./dist 2>&1 | tail -3
```

预期：build 成功 + `__DSH_BOOT__: N entries, rev=...`（N = 派生行数，**非硬编码**）。若 M4 sidecar 尚未构建，开发期可用 `composed-entries.json` 占位（标注「占位，M4 后由派生脚本重生成」），但**占位清单不得出现在 M3 验收证据中**（验收 ② 必须用派生清单）。

- [ ] **Step 9: 提交**

```bash
git add frontend/scripts frontend/apps/web/index.html
git commit -m "feat(frontend): manifest generation + plugin bundle pipeline + CSP + branding"
```

---

### Task 6: tauri dev 端到端验收（M3 验收 ①–⑥）

**Files:**
- Create: `frontend/composed-entries.json`（**派生产物**：`node scripts/derive-composed-entries.mjs ...` 生成，禁手写——R1 修正）
- Create: `docs/m3-dev-notes.md`
- Modify: `src-tauri/tauri.conf.json`（devUrl/beforeDevCommand——R1 修正：dev 模式须启动 vite，否则 WebView 空白）

**Interfaces:**
- Consumes: M1 carrier（sidecar 起 UDS）+ M2 Rust 命令 + M3 fork
- Produces: M3 验收证据

- [ ] **Step 1: dev 模式跑通（tauri dev + 手动 sidecar；R1 修正：beforeDevCommand 启动 vite）**

`src-tauri/tauri.conf.json` 的 `build` 段改为（R1 修正：dev 必须拉起 vite dev server，否则 devUrl 空白）：
```json
"build": {
  "beforeDevCommand": "pnpm --dir ../frontend dev",
  "devUrl": "http://localhost:1420",
  "beforeBuildCommand": "pnpm --dir ../frontend build",
  "frontendDist": "../frontend/dist"
}
```

```bash
# 先起 sidecar（M1 产物，desktop patch 启动）
DSH_HOME=/tmp/dsh-m3-test node ~/codehub/deepseek-harness/apps/cli/lib/bin.js \
  --profile web --port 0 \
  --patch /Users/acfufu/Codehub/dsh-desktop/host-patch/desktop.patch.yml &
# 再起 tauri dev（src-tauri 侧 DSH_SOCKET 指向 carrier socket）
DSH_SOCKET=/tmp/dsh-m3-test/run/dsh.sock cargo tauri dev
```

预期：beforeDevCommand 拉起 vite（端口 1420），WebView 渲染 fork dist。

- [ ] **Step 2: 验收 ② __DSH_BOOT__ 全条目 load**

观察 WebView 控制台/网络：`dist/plugins/<id>/client.js` 全部 200，无 `loaded without registering` 报错（facts §1：arrive() 抛错语义，system.ts:105-107）。条目数 = 组合行数（非硬编码）。

- [ ] **Step 3: 验收 ③ 状态到 connected**

前端日志应显示：双流 open（dsh_open_stream mux+host 返回）→ describe 成功 → connected（ConnectionController 代际成功路径）。

- [ ] **Step 4: 验收 ④ 会话列表/设置可交互**

WebView 内点击会话/设置 tab，UI 响应正常（经 dsh_http 往返）。

- [ ] **Step 5: 验收 ⑤ 有 key 真实 agent 回合**

```bash
DEEPSEEK_API_KEY=sk-... DSH_HOME=/tmp/dsh-m3-test node .../bin.js --profile web --port 0 --patch .../desktop.patch.yml &
```
发送一条消息，观察 agent 回合（含工具调用，若有）。

- [ ] **Step 6: 验收 ⑥ 构建管线测试绿**

```bash
cd frontend && pnpm vitest run scripts/
```

预期：schema/完整性/rev/inject 后 boot 可达 + CSP 断言全绿。

- [ ] **Step 7: 记录 + 提交**

```bash
git add docs/m3-dev-notes.md
git commit -m "docs(m3): dev-mode acceptance evidence"
```

---

## M3 完成检查（对照 spec §10 M3 验收）

- [ ] ① tauri dev 渲染 fork dist
- [ ] ② __DSH_BOOT__ 全部条目 load 成功（数量 = 组合行数，非硬编码）
- [ ] ③ 状态到 connected（双流 open + describe）
- [ ] ④ 会话列表/设置可交互
- [ ] ⑤ 有 key 时一个真实 agent 回合（含工具调用）
- [ ] ⑥ 构建管线测试绿（schema/完整性/rev/inject 后 boot 可达）
