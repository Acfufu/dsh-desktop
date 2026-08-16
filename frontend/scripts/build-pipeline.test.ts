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

  it('dist/index.html boot script is executable under its CSP (script-src sha256 matches inline script)', () => {
    const html = readFileSync(join(dist, 'index.html'), 'utf8');
    const scriptMatch = html.match(/<script>([^<]+)<\/script>/);
    expect(scriptMatch, 'boot script must be inline (injected by generate-manifest)').not.toBeNull();
    const body = scriptMatch![1]; // CSP hash 覆盖 <script> 与 </script> 之间的完整内容
    const { createHash } = require('node:crypto');
    const hash = createHash('sha256').update(body, 'utf8').digest('base64');
    // CSP 必须放行该内联脚本，否则 __DSH_BOOT__ 永不执行 → 白屏（回归：2026-08-16 实测）
    expect(html).toContain(`'sha256-${hash}'`); // unsafe-eval（loader 必需）夹在中间，只断言 hash 令牌
  });

  it('manifest carries immediately/inject tiers (runtime row must be immediately for cross-package require edges)', () => {
    const html = readFileSync(join(dist, 'index.html'), 'utf8');
    const m = html.match(/window\.__DSH_BOOT__ = (\{.*?\})<\/script>/s);
    const manifest = JSON.parse(m![1]);
    const rt = manifest.entries.find((e: { id: string }) => e.id === '@deepseek-ai/dsh-client-runtime');
    expect(rt?.immediately).toBe(true);
    expect(rt?.inject).toEqual(expect.arrayContaining(['@deepseek-ai/dsh-client-connection']));
  });

  it('dist/plugins entry count matches composed entries (non-hardcoded)', () => {
    const entries = JSON.parse(readFileSync(join(here, 'composed-entries.json'), 'utf8'));
    for (const { id } of entries) {
      const f = join(dist, `plugins/${id}/client.js`);
      expect(() => readFileSync(f)).not.toThrow();
    }
  });
});
