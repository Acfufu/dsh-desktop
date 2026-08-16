// 派生 __DSH_BOOT__ 条目清单（R1 修正：从磁盘 bundle 产物派生，禁硬编码行数）
// 输入：bundle 根目录（含各包 lib/client.js，支持 @scope/ 嵌套与 symlink）；输出：composed-entries.json
import { readFileSync, writeFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

const [,, bundleRoot, outFile] = process.argv;
const entries = [];

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
  const content = readFileSync(clientJs);
  const rev = createHash('sha1').update(content).digest('hex').slice(0, 12);
  entries.push({ id: pkg.name, file: clientJs, rev });
}
writeFileSync(outFile, JSON.stringify(entries, null, 2));
console.log(`derived ${entries.length} entries → ${outFile}`);
