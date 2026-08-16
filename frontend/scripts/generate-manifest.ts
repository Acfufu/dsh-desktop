// __DSH_BOOT__ 自产（spec §4.3）：entries 从运行时最终组合派生（≈33 行，禁硬编码）。
// schema: { rev, entries: [{ id, url, rev, inject?, immediately? }] }（manifest.ts:50-69）
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';

export interface ManifestEntry {
  id: string;
  file: string; // lib/client.js 绝对路径
  rev: string;  // sha1-12
  inject?: string[];
  immediately?: boolean;
}

export interface BootManifest {
  rev: string;
  entries: Array<{ id: string; url: string; rev: string; inject?: string[]; immediately?: boolean }>;
}

export function revOf(content: Buffer): string {
  return createHash('sha1').update(content).digest('hex').slice(0, 12);
}

// CSP script hash：内联 boot script 的 sha256-base64（CSP 'sha256-…' 精确放行，保持 script-src 'self' 严格）
export function scriptHashOf(content: string): string {
  return createHash('sha256').update(content, 'utf8').digest('base64');
}

export function bootScriptFor(manifest: BootManifest): string {
  // < 转义防 </script> 逃逸（DeepSec L3）
  const safe = JSON.stringify(manifest).replace(/</g, '\u003c');
  return `<script>window.__DSH_BOOT__ = ${safe}</script>`;
}

/** 注入 boot script 并给 CSP script-src 追加其 sha256（内联 script 无 hash 会被 script-src 'self' 拦截 → 白屏）。 */
export function injectBootWithCsp(html: string, script: string): string {
  const hash = scriptHashOf(script.slice('<script>'.length, -'</script>'.length));
  const head = html.indexOf('<head>');
  const cspPatched = html.replace(
    /script-src 'self' 'unsafe-eval'/,
    `script-src 'self' 'unsafe-eval' 'sha256-${hash}'`,
  );
  const target = cspPatched;
  if (head !== -1) return `${target.slice(0, head + 6)}${script}${target.slice(head + 6)}`;
  return `${script}${target}`;
}

export function buildManifest(entries: ManifestEntry[]): BootManifest {
  const composed = entries.map((e) => ({
    id: e.id,
    url: `/plugins/${e.id}/client.js?rev=${e.rev}`,
    rev: e.rev,
    ...(e.inject !== undefined && e.inject.length > 0 ? { inject: e.inject } : {}),
    ...(e.immediately === true ? { immediately: true } : {}),
  }));
  const rev = revOf(Buffer.from(JSON.stringify(composed.map((e) => e.rev))));
  return { rev, entries: composed };
}

// 收集入口：sidecar 或 npm 构建产物的 client bundle 目录（同一版本源，spec §4.3）
export function collectEntries(bundleRoot: string, ids: string[]): ManifestEntry[] {
  return ids.map((id) => {
    const file = join(bundleRoot, id, 'lib', 'client.js');
    const content = readFileSync(file);
    return { id, file, rev: revOf(content) };
  });
}

// 主流程：读组合清单（由运行时最终组合派生，M1 spike 产物或 sidecar 导出）→ 拷贝 → 注入
export function runMain() {
  const manifestFile = process.argv[2] ?? './composed-entries.json';
  const distRoot = process.argv[3] ?? './dist';
  const entries: Array<{ id: string; file: string; inject?: string[]; immediately?: boolean }> = JSON.parse(readFileSync(manifestFile, 'utf8'));
  const full = entries.map((e) => {
    const content = readFileSync(e.file);
    const rev = revOf(content);
    const dest = join(distRoot, 'plugins', e.id, 'client.js');
    mkdirSync(dirname(dest), { recursive: true });
    copyFileSync(e.file, dest);
    return {
      id: e.id, file: e.file, rev,
      ...(e.inject !== undefined && e.inject.length > 0 ? { inject: e.inject } : {}),
      ...(e.immediately === true ? { immediately: true } : {}),
    };
  });
  const manifest = buildManifest(full);
  const htmlPath = join(distRoot, 'index.html');
  const html = readFileSync(htmlPath, 'utf8');
  const script = bootScriptFor(manifest);
  const injected = injectBootWithCsp(html, script);
  writeFileSync(htmlPath, injected);
  console.log(`__DSH_BOOT__: ${full.length} entries, rev=${manifest.rev}`);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  runMain();
}
