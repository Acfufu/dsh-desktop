// 假 sidecar：UDS HTTP server，供 M2 单测/手动验证（真实 sidecar M4 替换）
import { createServer } from 'node:http';
import { mkdirSync, rmSync } from 'node:fs';

const path = process.env.DSH_SOCKET ?? '/tmp/dsh-uds-test/dsh.sock';
const dir = path.slice(0, path.lastIndexOf('/'));
mkdirSync(dir, { recursive: true, mode: 0o700 });
rmSync(path, { force: true }); // 清残留 socket（前次测试 kill 后遗留 → EADDRINUSE）

const server = createServer((req, res) => {
  if (req.url === '/api/host.describe') {
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ ok: true, method: 'host.describe' }));
    return;
  }
  res.statusCode = 404;
  res.end(JSON.stringify({ ok: false, error: 'not found' }));
});
server.listen(path, () => {
  console.log(`fake sidecar listening on ${path}`);
});
import { WebSocketServer } from 'ws';

const wss = new WebSocketServer({ noServer: true });
server.on('upgrade', (req, socket, head) => {
  const pathname = new URL(req.url ?? '/', 'http://dsh').pathname;
  if (pathname === '/api/events.mux' || pathname === '/api/events.host') {
    wss.handleUpgrade(req, socket, head, (ws) => {
      ws.send(JSON.stringify({ type: 'server-request', rpcId: 'fake-1', method: 'host.describe', payload: { ok: true } }));
      ws.on('message', () => {});
    });
  } else {
    socket.destroy();
  }
});
process.on('SIGTERM', () => { server.close(() => process.exit(0)); });
process.on('SIGINT', () => { server.close(() => process.exit(0)); });
