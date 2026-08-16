// fork 专用：connection 包 client bundle（transport = TauriApiClient，非上游 WebApiClient）
// 复刻上游 clientConfig（tsdown.client.ts）——entry 指向 fork 源码 src/client/index.ts
import { defineConfig } from 'tsdown'
import { CLIENT_EXTERNALS } from '../../../tsdown.client.ts'

export default defineConfig({
  name: '@deepseek-ai/dsh-client-connection/client',
  entry: { client: 'src/client/index.ts' },
  outDir: 'lib',
  format: 'cjs',
  platform: 'browser',
  target: 'es2024',
  dts: false,
  sourcemap: true,
  clean: false,
  external: [...CLIENT_EXTERNALS],
  noExternal: (id: string) => (CLIENT_EXTERNALS.includes(id) ? undefined : true),
  define: {
    'process.env.NODE_ENV': JSON.stringify(process.env.NODE_ENV ?? 'production'),
    'import.meta.env.MODE': JSON.stringify(process.env.NODE_ENV ?? 'production'),
    'import.meta.env': JSON.stringify({ MODE: process.env.NODE_ENV ?? 'production' }),
  },
  outputOptions: {
    entryFileNames: 'client.js',
    banner: `window.__ModuleLoader__.load({ id: "@deepseek-ai/dsh-client-connection", factory: (require) => {`,
    footer: 'return module.exports; } });',
    intro: 'var module = { exports: {} }; var exports = module.exports;',
  },
})
