// 把 ≈33 个 lib/client.js 拷到 dist/plugins/<id>/client.js（清单从运行时组合派生，非硬编码）
import { readFileSync, mkdirSync, copyFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';

const [,, bundleRoot, distRoot, entriesFile] = process.argv;
const entries = JSON.parse(readFileSync(entriesFile, 'utf8')); // [{id, file}]
for (const { id, file } of entries) {
  if (!existsSync(file)) throw new Error(`missing bundle: ${file}`);
  const dest = join(distRoot, 'plugins', id, 'client.js');
  mkdirSync(dirname(dest), { recursive: true });
  copyFileSync(file, dest);
}
console.log(`collected ${entries.length} bundles into ${distRoot}/plugins`);
