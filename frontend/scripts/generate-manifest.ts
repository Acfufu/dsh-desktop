// __DSH_BOOT__ 自产（spec §4.3）：entries 从运行时最终组合派生（≈33 行，禁硬编码）。
// schema: { rev, entries: [{ id, url, rev, inject?, immediately? }] }（manifest.ts:50-69）
import { createHash } from 'node:crypto';
import { injectBootManifest } from '@deepseek-ai/dsh-client-modules';
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';

export interface ManifestEntry {
  id: string;
  file: string; // lib/client.js 绝对路径
  rev: string;  // sha1-12
}

export interface BootManifest {
  rev: string;
  entries: Array<{ id: string; url: string; rev: string }>;
}

export function revOf(content: Buffer): string {
  return createHash('sha1').update(content).digest('hex').slice(0, 12);
}

export function buildManifest(entries: ManifestEntry[]): BootManifest {
  const composed = entries.map((e) => ({
    id: e.id,
    url: `/plugins/${e.id}/client.js?rev=${e.rev}`,
    rev: e.rev,
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
  const entries: Array<{ id: string; file: string }> = JSON.parse(readFileSync(manifestFile, 'utf8'));
  const full = entries.map((e) => {
    const content = readFileSync(e.file);
    const rev = revOf(content);
    const dest = join(distRoot, 'plugins', e.id, 'client.js');
    mkdirSync(dirname(dest), { recursive: true });
    copyFileSync(e.file, dest);
    return { id: e.id, file: e.file, rev };
  });
  const manifest = buildManifest(full);
  const htmlPath = join(distRoot, 'index.html');
  const html = readFileSync(htmlPath, 'utf8');
  const injected = injectBootManifest(html, manifest);
  writeFileSync(htmlPath, injected);
  console.log(`__DSH_BOOT__: ${full.length} entries, rev=${manifest.rev}`);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  runMain();
}
