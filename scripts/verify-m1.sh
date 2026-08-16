#!/usr/bin/env bash
# M1 验收脚本：无 TCP 监听、socket 权限、curl uplink、WS upgrade。
# 用法：先启动 sidecar（见 host-patch/README.md），再跑本脚本。
# 说明：本机 reach-guard 拦截 curl 外联，curl --unix-socket 用 node http.request 等价替代。
set -uo pipefail
SOCK="${1:-${DSH_HOME:-/tmp/dsh-m1-test}/run/dsh.sock}"
echo "== socket: $SOCK"

echo "== [1/4] 无 TCP 监听（本进程除外）"
lsof -iTCP -sTCP:LISTEN -P -n 2>/dev/null | grep -i "node" | grep -v TokenTracker || echo "PASS: no dsh TCP listen"

echo "== [2/4] socket 权限"
stat -f "%Sp %Su" "$SOCK"
stat -f "%Sp" "$(dirname "$SOCK")"

echo "== [3/4] uplink host.describe"
node -e "
const { request } = require('node:http');
const body = JSON.stringify({type:'client-request',rpcId:'m1-accept',method:'host.describe',payload:{}});
const req = request({ socketPath: process.argv[1], path: '/api/host.describe', method: 'POST', headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body) } }, (res) => {
  let data = '';
  res.on('data', (c) => data += c);
  res.on('end', () => { console.log('HTTP', res.statusCode); console.log(data.slice(0, 200)); process.exit(res.statusCode === 200 ? 0 : 1); });
});
req.on('error', (e) => { console.log('ERR', e.message); process.exit(1); });
req.end(body);
" "$SOCK"

echo "== [4/4] WS upgrade"
node -e "
const { request } = require('node:http');
const sock = process.argv[1];
let ok = 0;
function tryUpgrade(streamPath) {
  return new Promise((resolve) => {
    const req = request({ socketPath: sock, path: streamPath, method: 'GET', headers: { Upgrade: 'websocket', Connection: 'Upgrade', 'Sec-WebSocket-Key': 'dGhlIHNhbXBsZSBub25jZQ==', 'Sec-WebSocket-Version': '13' } });
    req.on('upgrade', (res, socket) => { console.log(streamPath, '→ UPGRADE OK', res.statusCode); socket.destroy(); ok++; resolve(); });
    req.on('response', (res) => { console.log(streamPath, '→ HTTP', res.statusCode); resolve(); });
    req.on('error', (e) => { console.log(streamPath, '→ ERROR', e.code); resolve(); });
    req.end();
  });
}
(async () => {
  await tryUpgrade('/api/events.mux');
  await tryUpgrade('/api/events.host');
  process.exit(ok === 2 ? 0 : 1);
})();
" "$SOCK"
