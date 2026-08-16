import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
describe('build pipeline output', () => {
  // vitest 下 import.meta.url 非 file scheme——用 cwd（vitest 以 frontend/ 为 cwd 运行）
  const here = process.cwd();
  const dist = join(here, 'dist');

  it('dist/index.html contains CSP meta with connect-src ipc:', () => {
    const html = readFileSync(join(dist, 'index.html'), 'utf8');
    expect(html).toContain('Content-Security-Policy');
    expect(html).toContain('connect-src');
    expect(html).toContain('ipc:');
  });

  it('dist/index.html contains boot manifest script (injectBootManifest inline, nonce removed per evidence)', () => {
    const html = readFileSync(join(dist, 'index.html'), 'utf8');
    expect(html).toContain('__DSH_BOOT__');
  });

  it('dist/plugins entry count matches composed entries (non-hardcoded)', () => {
    const entries = JSON.parse(readFileSync(join(here, 'composed-entries.json'), 'utf8'));
    for (const { id } of entries) {
      const f = join(dist, `plugins/${id}/client.js`);
      expect(() => readFileSync(f)).not.toThrow();
    }
  });
});
