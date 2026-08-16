// 派生 __DSH_BOOT__ 条目清单（R1 修正：从磁盘 bundle 产物派生，禁硬编码行数）
// 输入：bundle 根目录（含各包 lib/client.js，支持 @scope/ 嵌套与 symlink）；输出：composed-entries.json
import { readFileSync, writeFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

const [,, bundleRoot, outFile] = process.argv;
const entries = [];

// 桌面组合选择（与 host-patch/desktop.patch.yml 的 picker seam 一致：native host 插入、browse host 禁用）。
// 排除 browse client：它只调用 browse host 服务（ctx.workspaces.listDirectory，桌面未启用），且与 native
// 争用 conversation.hero.workspace.directoryFlow / sidebar.workspaces.directoryFlow 两个 single slot —
// 双注册同 priority 0 → 第二注册 throw → loader entry apply 失败 → boot 失败（白屏）。
// 同步纪律：改 picker 选择须同时改 desktop.patch.yml 与这里的排除表。
const DESKTOP_EXCLUDED_CLIENT_IDS = new Set([
  '@deepseek-ai/dsh-client-ui-directory-picker-browse',
]);

function candidateDirs(root) {
  const out = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (entry.name.startsWith('@')) {
      const scopeDir = join(root, entry.name);
      if (!statSync(scopeDir).isDirectory()) continue;
      for (const sub of readdirSync(scopeDir, { withFileTypes: true })) {
        out.push(join(scopeDir, sub.name));
      }
    } else if (entry.isDirectory() || entry.isSymbolicLink()) {
      out.push(join(root, entry.name));
    }
  }
  return out;
}

for (const dir of candidateDirs(bundleRoot)) {
  const pkgJson = join(dir, 'package.json');
  const clientJs = join(dir, 'lib', 'client.js');
  if (!existsSync(pkgJson) || !existsSync(clientJs)) continue;
  const pkg = JSON.parse(readFileSync(pkgJson, 'utf8'));
  if (!pkg.dsh?.client) continue; // 只收声明 dsh.client 的包
  if (DESKTOP_EXCLUDED_CLIENT_IDS.has(pkg.name)) continue; // 桌面组合排除（见上）
  const content = readFileSync(clientJs);
  const rev = createHash('sha1').update(content).digest('hex').slice(0, 12);
  const decl = pkg.dsh?.client ?? {};
  entries.push({
    id: pkg.name,
    file: clientJs,
    rev,
    ...(Array.isArray(decl.inject) && decl.inject.length > 0 ? { inject: decl.inject } : {}),
    ...(decl.immediately === true ? { immediately: true } : {}),
  });
}
// fork 覆盖：connection 包 transport 必须用 fork 构建（TauriApiClient）
const FORK_OVERRIDES = {
  '@deepseek-ai/dsh-client-connection': { root: 'packages/client/connection', file: 'lib/client.js' }, // cwd = frontend
};
for (const e of entries) {
  const o = FORK_OVERRIDES[e.id];
  if (o !== undefined) {
    const forkFile = join(process.cwd(), o.root, o.file);
    if (existsSync(forkFile)) {
      e.file = forkFile;
      e.rev = createHash('sha1').update(readFileSync(forkFile)).digest('hex').slice(0, 12);
    }
  }
}
writeFileSync(outFile, JSON.stringify(entries, null, 2));
console.log(`derived ${entries.length} entries → ${outFile}`);
