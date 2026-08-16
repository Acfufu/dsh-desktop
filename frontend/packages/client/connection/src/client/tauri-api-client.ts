import { invoke, Channel } from '@tauri-apps/api/core';
import { AbstractApiClient, IApiClient, RpcId } from './api';
import { serverRequestSchema } from '@deepseek-ai/dsh-host-apiproxy/api';
import type { RpcRequest, MuxFrame, HostFrame, ServerRequest } from './api';

// 传输错误 vs 业务错误分类（spec §4.3）：invoke reject（连接拒绝/IO/超时）→ 可重试传输错误；
// HTTP status + body → 业务错误（由 doFetch 内构造 Response 返回）。
export class TransportError extends Error {
  kind: 'transport' | 'business';
  constructor(kind: 'transport' | 'business', message: string, cause?: unknown) {
    super(message, { cause });
    this.kind = kind;
    this.name = 'TransportError';
  }
}

export class TauriApiClient extends AbstractApiClient implements IApiClient {
  constructor() {
    super(30_000); // 基类默认 30s + caller-signal-only 语义保留
  }

  protected async doFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
    // 前端 AbortSignal 映射为 Rust 侧取消（spec §4.3）：v1 简化——invoke 不挂起时由 dsh_cancel(id) 取消；
    // 此处 doFetch 无自身超时（调用点施加）。
    const method = (init?.method ?? 'GET').toUpperCase();
    // R2 修正：保留 query string（pathname 会丢 session.export?xxx 等参数）
    const url = new URL(input.toString(), 'http://dsh');
    const path = url.pathname + url.search;
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
      throw new TransportError('transport', `dsh_http failed: ${JSON.stringify(e)}`, e);
    }

    // 响应字节以 ArrayBuffer 保真重建 Response body（附件图片等二进制走此路，spec §4.3）
    const bytes = new Uint8Array(resp.body);
    const responseBody = new Blob([bytes]).arrayBuffer();
    return new Response(await responseBody, {
      status: resp.status,
      headers: new Headers(resp.headers),
    });
  }

  protected override openMux(
    _payload: { since?: Record<string, number> },
    signal: AbortSignal,
    onOpen?: () => void,
  ): AsyncIterable<RpcRequest<MuxFrame>> {
    return this.openDownlink<MuxFrame>('mux', signal, onOpen);
  }

  protected override openHost(
    _payload: { since?: Record<string, number> },
    signal: AbortSignal,
    onOpen?: () => void,
  ): AsyncIterable<RpcRequest<HostFrame>> {
    return this.openDownlink<HostFrame>('host', signal, onOpen);
  }

  private async *openDownlink<F extends MuxFrame | HostFrame>(
    stream: 'mux' | 'host',
    signal: AbortSignal,
    onOpen?: () => void,
  ): AsyncGenerator<RpcRequest<F>> {
    // 先创建 Channel 并注册 onmessage，再 invoke（invoke 返回即 onOpen 信号，spec §4.3）
    const channel = new Channel<string>();
    const frames: Array<RpcRequest<F>> = [];
    let endResolve: () => void = () => {};
    let ended = false;
    const endPromise = new Promise<void>((r) => { endResolve = r; });
    let notify: () => void = () => {};
    let pending = false;

    channel.onmessage = (text: string) => {
      if (text === '') { ended = true; endResolve(); return; }
      try {
        const full = serverRequestSchema.parse(JSON.parse(text)) as ServerRequest;
        this.onEnvelope?.(full); // 逐帧 onEnvelope tap（settings/credentials 安全观察，spec §4.3）
        const req: RpcRequest<F> = { rpcId: RpcId(full.rpcId), payload: full.payload as F };
        frames.push(req);
        pending = true;
        notify();
      } catch {
        // 单帧异常不得杀 generator（非 envelope 帧/坏 JSON 跳过）
      }
    };

    // 挂起的 open_stream invoke 绑定代际 AbortSignal（spec §4.3）——invoke 前注册 abortHandler，
    // 否则 signal 在 invoke 挂起期间触发时永不处理（generator 卡死）
    const abortHandler = () => { ended = true; endResolve(); };
    signal.addEventListener('abort', abortHandler, { once: true });

    let streamId: number;
    try {
      streamId = await invoke<number>('dsh_open_stream', { stream, channel });
    } catch (e) {
      signal.removeEventListener('abort', abortHandler);
      throw new TransportError('transport', `open stream ${stream} failed: ${JSON.stringify(e)}`, e);
    }

    // invoke 返回后检查是否已 abort（signal 在 invoke 期间触发过则直接结束）
    onOpen?.();
    if (signal?.aborted) { ended = true; endResolve(); }

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
      signal.removeEventListener('abort', abortHandler);
      await invoke('dsh_close_stream', { id: streamId }).catch(() => {});
    }
  }
}
