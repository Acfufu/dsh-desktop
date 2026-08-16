import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
// R2 修正：真扫 fork 源码，断言无 plugin-fs/shell/dialog 的 import（§4.5 排除原则的机器可验版本）
// DeepSec L3：Tauri v2 capability ACL 不门控 app 自定义命令——XSS 闸门是 CSP + on_navigation + asset scope；
// 本测试保持「源码不引入 fs/shell/dialog 插件」断言（纵深），CSP 断言见 build-pipeline。
describe('capability whitelist contract', () => {
  // vitest 下 import.meta.url 非 file scheme——cwd 即 frontend 根
  const root = process.cwd();

  // 扫描面 = fork 源码（bundle 产物含内联第三方死代码，如测试凭据串——非本仓库 invoke 面）
  function walk(dir: string, acc: string[] = []): string[] {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory() && !['node_modules', 'dist', 'public', 'lib'].includes(e.name)) walk(p, acc);
      else if (/\.[cm]?[jt]sx?$/.test(e.name) && !e.name.endsWith('.test.ts') && !e.name.endsWith('.test.tsx')) acc.push(p);
    }
    return acc;
  }

  it('fork source imports no fs/shell/dialog plugins', () => {
    const files = walk(root);
    const forbidden = ['@tauri-apps/plugin-fs', '@tauri-apps/plugin-shell', '@tauri-apps/plugin-dialog'];
    for (const f of files) {
      const src = readFileSync(f, 'utf8');
      for (const dep of forbidden) {
        expect(src, `${f} must not import ${dep}`).not.toContain(dep);
      }
    }
  });

  it('transport commands are the only dsh_* invokes', () => {
    const files = walk(root);
    const allowed = new Set(['dsh_http', 'dsh_open_stream', 'dsh_close_stream', 'dsh_cancel', 'dsh_save_export', 'dsh_write_temp', 'dsh_import_dropped', 'dsh_export_session']);
    for (const f of files) {
      const src = readFileSync(f, 'utf8');
      for (const m of src.matchAll(/invoke\s*\(\s*['"]([^'"]+)['"]/g)) {
        expect(allowed.has(m[1]), `${f} invokes non-whitelisted ${m[1]}`).toBe(true);
      }
    }
  });
});
