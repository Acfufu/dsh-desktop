/** Desktop caller for generic Connection unary RPC channels (invoke 经 Rust 哑管道). */

import {
  RpcId,
  serverResponseSchema,
  type ClientRequest,
} from '@deepseek-ai/dsh-host-apiproxy/api'
import { invoke } from '@tauri-apps/api/core'
import type { ClientConnectionRpc } from '../rpc.ts'
import { randomUuid } from './random-uuid.ts'

const CHANNEL_PATTERN = /^\/[A-Za-z0-9._~-]+$/
const ENDPOINT_SEGMENT_PATTERN = /^[A-Za-z0-9_$.-]+$/

/**
 * Create the browser-backed generic RPC caller.
 * @returns caller that owns request correlation and response-envelope validation.
 */
export function createWebConnectionRpc(): ClientConnectionRpc {
  return {
    async call(channel, endpoint, payload, signal) {
      assertTarget(channel, endpoint)
      const rpcId = RpcId(randomUuid())
      const message: ClientRequest = {
        type: 'client-request',
        rpcId,
        method: endpoint,
        payload,
      }
      const resp = await postEnvelope(channel, endpoint, message, signal)
      const full = serverResponseSchema.parse(resp)
      if (full.rpcId !== rpcId) {
        throw new Error(`rpcId mismatch for ${endpoint}: sent ${rpcId}, got ${full.rpcId}`)
      }
      return full.result
    },
  }
}

// 原实现用 globalThis.fetch（tauri://localhost 不可用，spec §4.3）——换 invoke dsh_http。
// 签名保持 call(channel, endpoint, payload, signal?)，不得设超时（command.execute 经此通道）。
async function postEnvelope(channel: string, endpoint: string, envelope: unknown, signal?: AbortSignal): Promise<unknown> {
  if (signal?.aborted) throw new DOMException('Aborted', 'AbortError')
  // R1 修正：path = `${channel}/${endpoint}`（channel 已含前导 /，如 '/api'）——
  // 原写法 `/${channel}/${endpoint}` 会产生 '//api/...' 双斜杠，被 Rust 输入校验拒绝。
  const resp = await invoke<{ status: number; body: number[] }>('dsh_http', {
    method: 'POST',
    path: `${channel}/${endpoint}`,
    body: Array.from(new TextEncoder().encode(JSON.stringify(envelope))),
  })
  if (resp.status >= 400) {
    throw new Error(`rpc ${channel}/${endpoint} → HTTP ${resp.status}: ${new TextDecoder().decode(new Uint8Array(resp.body))}`)
  }
  // DeepSec L3：JSON.parse 本身安全（own property），但下游 Object.assign 可经 __proto__ 键污染原型——
  // 剥掉非法的 __proto__/constructor/prototype 键（server 信封永远不需要它们）
  const parsed: any = JSON.parse(new TextDecoder().decode(new Uint8Array(resp.body)))
  for (const k of ['__proto__', 'constructor', 'prototype']) {
    if (k in parsed) delete parsed[k]
  }
  return parsed
}

function assertTarget(channel: string, endpoint: string): void {
  const segments = endpoint.split('/')
  if (!CHANNEL_PATTERN.test(channel)
    || segments.some(segment =>
      segment === '' || segment === '.' || segment === '..' || !ENDPOINT_SEGMENT_PATTERN.test(segment))) {
    throw new Error(`connection: invalid RPC target ${JSON.stringify(`${channel}/${endpoint}`)}`)
  }
}
