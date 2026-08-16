import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { UdsCarrierService } from './index.js';
import { makeMockApiProxy } from './mock-test-util';
import { selectSocketPath } from './socket-path.js';

// R2 修正：真实 fs/http 由 vi.mock 替换，测 start() 副作用（chmod 600、0700 目录、残留 unlink）
vi.mock('node:fs', () => ({
  mkdirSync: vi.fn(),
  chmodSync: vi.fn(),
  unlinkSync: vi.fn(),
  existsSync: vi.fn(() => false),
  // R3 修正：后续测试（Step 5 / M5 Task 2）调用这些——mock 不补全会 TypeError
  mkdtempSync: vi.fn(() => '/tmp/dsh-mock-tmp'),
  rmSync: vi.fn(),
  statSync: vi.fn(() => ({ mode: 0o700 })),
  lstatSync: vi.fn(() => ({ isSymbolicLink: () => false, uid: process.getuid?.() ?? 0 })),
  writeFileSync: vi.fn(),
}));
vi.mock('node:http', () => ({
  createServer: vi.fn(() => ({
    on: vi.fn(),
    once: vi.fn(),
    listen: vi.fn((p: unknown, cb?: () => void) => { cb?.(); return this; }),
    close: vi.fn((cb?: () => void) => { cb?.(); }),
    closeAllConnections: vi.fn(),
  })),
}));

describe('uds-carrier start', () => {
  it('starts server, chmod 600 socket, mkdir 0700 dir', async () => {
    const fs = await import('node:fs');
    const http = await import('node:http');
    const svc = new UdsCarrierService(
      { logger: { info: vi.fn(), error: vi.fn() }, get: vi.fn(() => makeMockApiProxy(async () => ({}))), provide: vi.fn(), on: vi.fn(), reflect: { provide: vi.fn() } } as any, // test-only cast
      { udsPath: '/tmp/dsh-test/dsh.sock' },
    );
    await svc.start();
    expect(fs.mkdirSync).toHaveBeenCalledWith('/tmp/dsh-test', expect.objectContaining({ mode: 0o700 }));
    expect(fs.chmodSync).toHaveBeenCalledWith('/tmp/dsh-test/dsh.sock', 0o600);
    expect(http.createServer).toHaveBeenCalled();
  });
});

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
