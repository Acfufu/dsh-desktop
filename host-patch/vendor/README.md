# Vendored files

| File | Upstream (commit 47f94385) | Reason |
|---|---|---|
| `packages/uds-carrier/vendor/http-bridge.ts` | packages/client/connection/src/http-bridge.ts | `bridge` 不在 npm 导出面（仅 ./src/*，npm files 不含 src） |
| `packages/uds-carrier/vendor/websocket-downlink.ts` | packages/client/connection/src/websocket-downlink.ts | `WebSocketDownlinks` 不在导出面 |

同步：`scripts/sync-carrier.sh`（cmp 逐字节校验，vendor 文件不得手改——头部/注释一律写本文件）。
