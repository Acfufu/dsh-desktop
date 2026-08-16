/**
 * Web application entry: thin bootstrap over the shell library. Everything —
 * loader holding, module-table seeding, AppRoot gate, plugin assembly — lives
 * in @deepseek-ai/dsh-client-web; this file only finds the mount point.
 */
import { AppWebEntry } from '@deepseek-ai/dsh-client-web'

// 拦截 target=_blank：模型输出 markdown 链接点击不得导航主窗口（spec §4.2）→ 系统浏览器
document.addEventListener('click', (e) => {
  const a = (e.target as HTMLElement).closest?.('a[target="_blank"]');
  if (a) {
    e.preventDefault();
    const href = (a as HTMLAnchorElement).href;
    if (href.startsWith('http')) {
      void import('@tauri-apps/plugin-opener').then(({ openUrl }) => openUrl(href));
    }
  }
}, true);

const el = document.getElementById('root')
if (el === null) throw new Error('web app: missing #root')
void new AppWebEntry(el).run()
