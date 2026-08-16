// scripts/bench-big-body.mjs：150 MiB UDS HTTP 服务（bench 测试 spawn 用）
import { createServer } from 'node:http';
import { mkdirSync } from 'node:fs';
const path = process.env.DSH_SOCKET ?? '/tmp/dsh-uds-test/dsh.sock';
mkdirSync(path.slice(0, path.lastIndexOf('/')), { recursive: true, mode: 0o700 });
const SIZE = 150 * 1024 * 1024;
const chunk = Buffer.alloc(64 * 1024, 0x61);
const server = createServer((req, res) => {
  let sent = 0;
  res.writeHead(200, { 'Content-Type': 'application/octet-stream' });
  const timer = setInterval(() => {
    res.write(chunk);
    sent += chunk.length;
    if (sent >= SIZE) { clearInterval(timer); res.end(); }
  }, 5);
});
server.listen(path, () => console.log(`big-body server on ${path}`));
process.on('SIGTERM', () => server.close(() => process.exit(0)));
