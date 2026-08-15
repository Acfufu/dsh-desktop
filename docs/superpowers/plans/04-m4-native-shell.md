# M4 — 原生壳 + 打包 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> 共享事实底座：先读 `docs/superpowers/plans/00-verified-facts-and-corrections.md`（版本、符号、修正以它为准）。

**Goal:** 完成原生壳层：托盘显隐/退出、自启 LaunchAgent、agent 完成通知（前端经 plugin-notification 直调）、外链走系统浏览器且 target=_blank 不导航主窗口、>10 MiB 附件/导出走既定路径（§4.6 临时文件纪律）、非白名单 invoke 被拒、frontendDist 缺失弹错误对话框、sidecar 真实打包（node + 包目录布局，spec §4.4）、.app 签名+公证（arm64）、identifier 定稿。

**Architecture:** Rust 侧把 M2 的进程管理器接真实 sidecar（`bin/node lib/bin.js --profile web --patch <abs>/patch/desktop.patch.yml`，cwd=`$DSH_HOME`，setpgid），导航白名单谓词抽纯函数，托盘/自启/opener/单实例/通知插件全部接线；sidecar 资源按 `Resources/dsh/` 布局（bin/node + package.json + lib/ + config/ + 裁剪 node_modules + patch/）；>10 MiB 文件交接按 §4.6（上传拖拽路径 + 下载落 Downloads + asset protocol 严格 scope）。

**Tech Stack:** tauri 2.11.5 + 四插件（single-instance 2.4.3 / autostart 2.5.1 / notification 2.3.3 / opener 2.5.4）、tauri-plugin-shell **不引入**（§4.5 排除原则）、lipo（架构定案时）、`cargo bundle`（tauri build）。

## Global Constraints

- 执行者 = deepseek-v4-flash 级别：每步完整代码/命令/预期输出；禁止「参考上一步」。
- **排除原则（§4.5）：不引入 fs / shell / dialog / http 插件（opener 除外），不暴露任何非 transport 命令。**
- `exitOnLastWindowClosed: false`（关闭窗口 = 隐藏到托盘）。
- 导航白名单：`allowed_navigation(url, debug)` 纯函数，四象限单测。
- assetProtocol scope **严格限定 app 专属临时目录**（绝不含 $HOME/全盘）——scope 设错则 XSS → 任意文件读（§4.6）。
- 通知：前端经 IPC 调 plugin-notification（Rust 不解析协议）；macOS 无需 Info.plist 权限声明，capability 加 `notification:default`。
- App Sandbox：v1 不做（§4.2）；hardened runtime + 公证。
- 版本 pin：facts §6。lipo/架构在 M4 定案（arm64 先行）。

---

### Task 1: 托盘 + 窗口隐藏 + 单实例

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tauri.conf.json`（exitOnLastWindowClosed、tray）
- Create: `src-tauri/src/tray.rs`

**Interfaces:**
- Consumes: tauri 内置 tray API（v2 无需插件）
- Produces: 托盘显隐/退出；`exitOnLastWindowClosed: false`；single-instance 弹「已在运行」

- [ ] **Step 1: 托盘模块（显隐/退出）**

`src-tauri/src/tray.rs`：
```rust
use tauri::{AppHandle, Manager, tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState}};

pub fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_i = tauri::menu::MenuBuilder::new(app)
        .text("show", "显示窗口")
        .text("quit", "退出")
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().unwrap())
        .menu(&show_i)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => { show_main_window(app); }
            "quit" => { app.exit(0); }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}
```

- [ ] **Step 2: lib.rs 接线（托盘 + 单实例 + 隐藏到托盘）**

`src-tauri/src/lib.rs` 扩展：
```rust
mod tray;

use tauri::Manager;

fn is_main_window_close(app: &tauri::AppHandle) {
    // exitOnLastWindowClosed:false 已配；窗口关闭时隐藏而非退出
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

pub fn run() {
    let uds_path = std::env::var("DSH_SOCKET").unwrap_or_else(|_| http_command::UDS_PATH.to_string());
    let http_client = reqwest::ClientBuilder::new()
        .unix_socket(&uds_path)
        .build()
        .expect("build reqwest client with unix socket");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 单实例：二次启动 → 显示已有窗口（spec §4.2）
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .manage(AppState {
            http_client,
            uds_path,
            registry: Arc::new(Mutex::new(StreamRegistry::new())),
        })
        .setup(|app| {
            tray::build_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 主窗口关闭 → 隐藏到托盘（不退出）
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![dsh_http, dsh_open_stream, dsh_close_stream, dsh_cancel])
        // R1 修正：退出序列接线必须保留（M2 Task 5 的 RunEvent::ExitRequested → prevent_exit →
        // SIGTERM(组) → 5s → SIGKILL(组) → unlink socket → exit）——本步 run() 若整体替换 lib.rs
        // 不得丢弃该钩子；完整实现见 Task 5 Step 4。
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
                tauri::async_runtime::spawn(async move {
                    crate::shutdown_sequence(app_handle).await; // Task 5 Step 4 定义
                });
            }
        });
}
```

- [ ] **Step 3: tauri.conf.json 追加（exitOnLastWindowClosed + 托盘图标）**

```json
  "app": {
    "windows": [ ... 同 M2 ... ],
    "security": { ... 同 M2 ... },
    "trayIcon": { "iconPath": "icons/icon.png", "iconAsTemplate": true },
    "exitOnLastWindowClosed": false
  },
```

- [ ] **Step 4: 编译 + 手动验证**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

预期：check 通过（托盘 API 与 v2.11 签名一致——若 `tauri::menu` 路径差异，按编译器提示调整）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/tray.rs src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "feat(src-tauri): tray show/hide/quit + single-instance + hide-to-tray"
```

---

### Task 2: 导航白名单 + 外链 opener

**Files:**
- Create: `src-tauri/src/navigation.rs`
- Create: `src-tauri/src/navigation.rs`（四象限单测）
- Modify: `src-tauri/src/lib.rs`（on_navigation 接线）
- Modify: `src-tauri/capabilities/desktop.json`（opener:default）

**Interfaces:**
- Consumes: `tauri-plugin-opener`（2.5.4）
- Produces: `fn allowed_navigation(url: &str, debug: bool) -> bool`——tauri://localhost ✓ / http://ipc.localhost ✓ / dev URL（debug 门控）✓ / 其余 ✗

- [ ] **Step 1: 写失败测试（四象限）**

`src-tauri/src/navigation.rs`：
```rust
/// 导航白名单（spec §4.2）：仅放行 tauri://localhost 与 http://ipc.localhost；
/// dev 构建额外放行 vite dev server（cfg!(debug_assertions) 门控）。
pub fn allowed_navigation(url: &str, debug: bool) -> bool {
    if url.starts_with("tauri://localhost") || url.starts_with("http://ipc.localhost") {
        return true;
    }
    if debug && (url.starts_with("http://localhost:1420") || url.starts_with("http://127.0.0.1:1420")) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_tauri_localhost() {
        assert!(allowed_navigation("tauri://localhost/", false));
        assert!(allowed_navigation("tauri://localhost/index.html", false));
    }

    #[test]
    fn allows_ipc_localhost() {
        assert!(allowed_navigation("http://ipc.localhost", false));
    }

    #[test]
    fn rejects_external_links() {
        assert!(!allowed_navigation("https://example.com", false));
        assert!(!allowed_navigation("https://evil.com/phish", false));
    }

    #[test]
    fn dev_url_only_in_debug() {
        assert!(allowed_navigation("http://localhost:1420/", true));
        assert!(!allowed_navigation("http://localhost:1420/", false));
    }

    #[test]
    fn rejects_file_scheme() {
        assert!(!allowed_navigation("file:///etc/passwd", false));
    }
}
```

- [ ] **Step 2: 运行确认通过**

```bash
cd src-tauri && cargo test navigation 2>&1 | tail -5
```

预期：5 passed。

- [ ] **Step 3: lib.rs 接线（on_navigation + opener）**

`src-tauri/src/lib.rs` 追加：
```rust
use tauri::webview::WebviewWindowBuilder;

// 在 setup 中创建窗口并挂 on_navigation（或对已有窗口设置）：
// v2: WebviewWindowBuilder::new(...).on_navigation(|url| allowed_navigation(url.as_str(), cfg!(debug_assertions)))
// 主窗口在 tauri.conf.json 定义时无法挂 on_navigation → 改为 builder 创建：
```

> 注：v2 中 conf 定义的窗口无法挂 on_navigation 回调——需改为 `WebviewWindowBuilder` 显式创建主窗口（`tauri.conf.json` 里删 `app.windows`，改由 setup 创建）。以下为 setup 版：
```rust
use tauri::webview::WebviewWindowBuilder;

// setup 内：
let win = WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
    .title("dsh-desktop")
    .inner_size(1200.0, 800.0)
    .on_navigation(|url| navigation::allowed_navigation(url.as_str(), cfg!(debug_assertions)))
    .build()?;
```

- [ ] **Step 4: 外链处理（fork 侧拦截 target=_blank + opener）**

前端 fork 加（`frontend/packages/client/web/src/app-shell.ts` 或 boot 层，最小实现——文档层拦截）：
```typescript
// 拦截 target=_blank：模型输出 markdown 链接点击不得导航主窗口（spec §4.2）
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
```

- [ ] **Step 5: capability 加 opener:default**

`src-tauri/capabilities/desktop.json` 的 permissions 数组追加 `"opener:default"`（M2 已有，确认存在）。

- [ ] **Step 6: 编译 + 测试 + 提交**

```bash
cd src-tauri && cargo test 2>&1 | tail -5 && cargo check 2>&1 | tail -3
git add src-tauri/src/navigation.rs src-tauri/src/lib.rs frontend/packages/client/web/src/app-shell.ts
git commit -m "feat(src-tauri): navigation whitelist + opener for external links"
```

---

### Task 3: 自启（autostart）+ 通知（前端直调）

**Files:**
- Modify: `src-tauri/src/lib.rs`（autostart 插件）
- Modify: `src-tauri/capabilities/desktop.json`（autostart 三权限）
- Modify: `frontend/packages/client/web/src/`（通知订阅，§4.3 通知模块）
- Create: `frontend/packages/client/web/src/notify.ts`
- Create: `frontend/packages/client/web/src/notify.test.ts`

**Interfaces:**
- Consumes: `tauri-plugin-autostart`（Rust）、`@tauri-apps/plugin-notification`（前端）
- Produces: 自启 enable/disable/isEnabled 三命令（前端调用）；agent 完成/提问事件 → 系统通知（`api.subscribeEnvelopes` 过滤 `turn/end` / `session-status`）

- [ ] **Step 1: Rust 侧 autostart 插件**

`src-tauri/src/lib.rs`：`.plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))` 加进 Builder chain。

- [ ] **Step 2: capability 权限**

`desktop.json` permissions 追加：
```json
"autostart:allow-enable",
"autostart:allow-disable",
"autostart:allow-is-enabled"
```

- [ ] **Step 3: 前端通知模块（写失败测试；R1 修正：subscribeEnvelopes 未实证——主路径用 fork 内 TauriApiClient 的 onEnvelope tap，见 spec §4.3「逐帧调用 onEnvelope tap」）**

`frontend/packages/client/web/src/notify.ts`：
```typescript
// 通知（spec §4.3）：订阅 agent 完成/提问事件。
// R1 修正：subscribeEnvelopes 未在任何核查中实证——改用 fork 内 TauriApiClient 的
// onEnvelope tap（facts §2 实证 openMux/openHost 逐帧调用 onEnvelope）作为事件源。
import { ServerRequest } from '@deepseek-ai/dsh-host-apiproxy';

// 事件源抽象：由 fork 的 transport 层（TauriApiClient）注入
// （R1 修正：不依赖未实证的 subscribeEnvelopes；若实测 @deepseek-ai/dsh-host-apiproxy
//  根确实导出 subscribeEnvelopes，可保留为兼容分支，但主路径是 onEnvelope）
export interface EnvelopeSource {
  subscribe(fn: (req: ServerRequest) => void): () => void;
}

export interface NotificationEvent {
  title: string;
  body: string;
}

// 纯函数：从 ServerRequest 判断是否触发通知（可测）
export function notificationFromRequest(req: ServerRequest): NotificationEvent | null {
  if (req.type !== 'server-request') return null;
  if (req.method === 'turn/end') {
    return { title: 'Agent 完成', body: '回合已结束' };
  }
  if (req.method === 'session-status' && (req.payload as any)?.status === 'question') {
    return { title: '需要输入', body: 'Agent 正在等你回应' };
  }
  return null;
}

export async function startNotifications(source: EnvelopeSource): Promise<() => void> {
  const { isPermissionGranted, requestPermission, sendNotification } = await import('@tauri-apps/plugin-notification');
  let granted = await isPermissionGranted();
  if (!granted) {
    granted = (await requestPermission()) === 'granted';
  }
  if (!granted) return () => {};

  return source.subscribe((req) => {
    const n = notificationFromRequest(req);
    if (n) sendNotification({ title: n.title, body: n.body });
  });
}
```

`frontend/packages/client/web/src/notify.test.ts`：
```typescript
import { describe, it, expect } from 'vitest';
import { notificationFromRequest } from './notify';

describe('notificationFromRequest', () => {
  it('fires on turn/end', () => {
    const r = { type: 'server-request', rpcId: 'r1', method: 'turn/end', payload: {} } as any;
    expect(notificationFromRequest(r)?.title).toBe('Agent 完成');
  });

  it('fires on session-status question', () => {
    const r = { type: 'server-request', rpcId: 'r2', method: 'session-status', payload: { status: 'question' } } as any;
    expect(notificationFromRequest(r)?.title).toBe('需要输入');
  });

  it('ignores unrelated methods', () => {
    const r = { type: 'server-request', rpcId: 'r3', method: 'session/list', payload: {} } as any;
    expect(notificationFromRequest(r)).toBeNull();
  });
});
```

- [ ] **Step 4: 运行测试 + boot 时启动订阅**

```bash
cd frontend && pnpm vitest run packages/client/web/src/notify.test.ts
```
预期：3 passed。在 boot（seed.ts 或 app-shell）connected 后调 `startNotifications()`。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/lib.rs src-tauri/capabilities/desktop.json frontend/packages/client/web/src/notify.ts frontend/packages/client/web/src/notify.test.ts
git commit -m "feat: autostart plugin + agent notification via frontend"
```

---

### Task 4: 临时文件纪律（>10 MiB 交接 + asset protocol scope）

**Files:**
- Create: `src-tauri/src/tempfiles.rs`
- Create: `src-tauri/src/tempfiles.rs`（单测：随机名、canonicalize 越界拒绝、age 清扫、退出序列⑤清理、下载落盘）
- Modify: `src-tauri/src/lib.rs`（命令注册）
- Modify: `src-tauri/tauri.conf.json`（assetProtocol scope 验证）

**Interfaces:**
- Consumes: `app.path().app_cache_dir()`；`asset protocol`（convertFileSrc）
- Produces:
  - `#[tauri::command] fn dsh_save_export(bytes: Vec<u8>, file_name: String) -> Result<String, String>`（落 ~/Downloads，随机名后缀防覆盖，弹通知）
  - `#[tauri::command] fn dsh_write_temp(bytes: Vec<u8>, ext: String) -> Result<String, String>`（app cache 子目录，0600，随机名）
  - `fn canonicalize_within(root: &Path, candidate: &Path) -> Result<PathBuf, String>`（越界拒绝）

- [ ] **Step 1: 写失败测试（canonicalize 越界拒绝 + 随机名）**

`src-tauri/src/tempfiles.rs`：
```rust
use std::path::{Path, PathBuf};

/// 上传/临时文件纪律（spec §4.6）：canonicalize 后校验仍在允许范围内。
pub fn canonicalize_within(root: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    let cand_c = candidate.canonicalize().map_err(|e| e.to_string())?;
    if cand_c.starts_with(&root_c) {
        Ok(cand_c)
    } else {
        Err(format!("path escapes allowed root: {}", cand_c.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn accepts_within_root() {
        let root = std::env::temp_dir().join("dsh-temp-test");
        fs::create_dir_all(&root).unwrap();
        let inner = root.join("a.txt");
        fs::write(&inner, "x").unwrap();
        assert!(canonicalize_within(&root, &inner).is_ok());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_escape() {
        let root = std::env::temp_dir().join("dsh-temp-test2");
        fs::create_dir_all(&root).unwrap();
        let outside = std::env::temp_dir().join("dsh-escape-marker.txt");
        fs::write(&outside, "x").unwrap();
        assert!(canonicalize_within(&root, &outside).is_err());
        fs::remove_dir_all(&root).unwrap();
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn random_name_no_user_input() {
        // 文件名由 Rust 随机生成（spec §4.6）——用 uuid v4（或时间戳+随机），绝不采用 Content-Disposition
        let name = format!("dsh-{}-{}.bin", std::process::id(), nanoid());
        assert!(name.starts_with("dsh-"));
    }

    fn nanoid() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        format!("{n:x}")
    }
}
```

- [ ] **Step 2: 运行确认通过**

```bash
cd src-tauri && cargo test tempfiles 2>&1 | tail -5
```

预期：3 passed。

- [ ] **Step 3: 实现命令（save_export + write_temp）**

`src-tauri/src/tempfiles.rs` 追加：
```rust
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn dsh_save_export(app: AppHandle, bytes: Vec<u8>, file_name: String) -> Result<String, String> {
    // 下载（spec §4.6）：session.export ZIP → 用户下载目录 + 系统通知
    let downloads = app.path().download_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    // 随机名后缀防覆盖；file_name 仅作前缀（sessionId 生成，非用户输入，仍加随机后缀兜底）
    let safe_name = format!("{}-{nanoid()}.zip", sanitize(&file_name));
    let path = downloads.join(&safe_name);
    fs::write(&path, &bytes).map_err(|e| format!("disk write failed: {e}"))?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub async fn dsh_write_temp(app: AppHandle, bytes: Vec<u8>, ext: String) -> Result<String, String> {
    // 上传临时文件（spec §4.6）：app 专属临时子目录，0600，随机名
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let dir = cache.join("temp-uploads");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut perms = fs::metadata(&dir).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&dir, perms).map_err(|e| e.to_string())?;
    let safe_ext = sanitize(&ext).chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>();
    let path = dir.join(format!("dsh-{nanoid()}.{safe_ext}"));
    fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

#[cfg(test)]
mod helpers {
    use super::sanitize;
    #[test]
    fn sanitize_strips_dangerous() {
        assert_eq!(sanitize("../evil/name"), "___evil_name");
    }
}
```

- [ ] **Step 4: tauri.conf.json asset scope（验证 $APPCACHE）**

```json
"assetProtocol": {
  "enable": true,
  "scope": ["$APPCACHE/temp-uploads/**", "$APPCACHE/temp/**"]
}
```

> 注：`$APPCACHE` 类路径变量的可用性在 M4 验证（spec §4.7 验证项）——若不可用，改用运行时 `app.path().app_cache_dir()` 物化的绝对路径注入 capability（Rust 侧动态创建 capability 或文档记录替代）。

- [ ] **Step 5: 上传/下载路径接线（前端）**

- 上传 <10 MiB：`File.arrayBuffer()` → `invoke('dsh_write_temp', ...)`；>10 MiB 仅拖拽（`onDragDropEvent` 给原生路径，Rust 直接读源文件，spec §4.6）。>10 MiB 且选择器选中 → 拒绝并提示。
- 下载：前端 `invoke('dsh_save_export', { bytes, file_name })`（session.export 流式读取完成后）→ 通知「已下载到 ~/Downloads」。

- [ ] **Step 6: 编译 + 测试 + 提交**

```bash
cd src-tauri && cargo test 2>&1 | tail -5 && cargo check 2>&1 | tail -3
git add src-tauri/src/tempfiles.rs src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "feat(src-tauri): temp file discipline + export download"
```

---

### Task 5: sidecar 打包（node + 包目录布局，§4.4）

**Files:**
- Create: `scripts/build-sidecar.sh`
- Create: `src-tauri/resources/dsh/`（构建产物占位，gitignore）
- Modify: `src-tauri/tauri.conf.json`（resources: ["dsh/**"]）

**Interfaces:**
- Consumes: DSH_REPO（`~/codehub/deepseek-harness`）、UPSTREAM_PIN
- Produces: `.app/Contents/Resources/dsh/` 布局（bin/node、package.json、lib/bin.js、config/agent-presets/、node_modules 裁剪、patch/desktop.patch.yml）

- [ ] **Step 1: build-sidecar.sh（spec §4.4 流程）**

`scripts/build-sidecar.sh`：
```bash
#!/usr/bin/env bash
# sidecar 构建（node 运行时 + 包目录布局，spec §4.4 主方案；bun compile 远期，不在本计划）
set -euo pipefail
REPO="${DSH_REPO:-$HOME/codehub/deepseek-harness}"
OUT="src-tauri/resources/dsh"
PIN="host-patch/UPSTREAM_PIN"
COMMIT="$(grep '^COMMIT=' "$PIN" | cut -d= -f2)"

[[ -d "$REPO" ]] || { echo "DSH_REPO not found at $REPO (set DSH_REPO)"; exit 1; }
git -C "$REPO" rev-parse HEAD | grep -q "^$COMMIT" || {
  echo "WARN: $REPO HEAD != $COMMIT; run 'git -C $REPO checkout $COMMIT' to match pin"; exit 1;
}

echo "==> pnpm install + build (web profile closure)"
cd "$REPO"
pnpm install 2>&1 | tail -2
pnpm run build 2>&1 | tail -2

echo "==> assemble $OUT"
mkdir -p "$OUT/bin" "$OUT/lib" "$OUT/config" "$OUT/patch"
NODE_BIN="$(command -v node)"
cp "$NODE_BIN" "$OUT/bin/node"                       # node 二进制（^22.19 || >=24；本机 24.14.1）
cp "$REPO/apps/cli/package.json" "$OUT/package.json" # INSTALL_ANCHOR 命中（lib/../package.json）
cp -r "$REPO/apps/cli/lib/." "$OUT/lib/"             # lib/bin.js + lib/types/
cp -r "$REPO/apps/cli/config/." "$OUT/config/"       # SHIPPED_PRESET_ROOT（lib/../config/agent-presets）
cp /Users/acfufu/Codehub/dsh-desktop/host-patch/desktop.patch.yml "$OUT/patch/desktop.patch.yml"

echo "==> node_modules 裁剪（web 闭包：pnpm --filter / deploy 或 npm pack 集合）"
# v1：从 DSH_REPO 复制 node_modules（大而全），后续用 pnpm deploy 裁剪优化（spec §4.4 提及）
cp -r "$REPO/node_modules" "$OUT/node_modules" 2>/dev/null || { echo "copying node_modules (large)"; cp -r "$REPO/node_modules" "$OUT/node_modules"; }
# 装入 uds-carrier 本地包（共享依赖树）——R1 修正：与 patch name 一致（@dsh-desktop/uds-carrier）
mkdir -p "$OUT/node_modules/@dsh-desktop"
cp -r /Users/acfufu/Codehub/dsh-desktop/host-patch/packages/uds-carrier "$OUT/node_modules/@dsh-desktop/uds-carrier"

echo "==> verify"
"$OUT/bin/node" -e "console.log('node ok', process.version)"
ls "$OUT"
echo "sidecar assembled (pin $COMMIT). Size: $(du -sh "$OUT" | cut -f1)"
```

- [ ] **Step 2: 构建 + 校验**

```bash
chmod +x scripts/build-sidecar.sh && ./scripts/build-sidecar.sh 2>&1 | tail -15
```

预期：`node ok v24.14.1` + 目录清单 + 体积 ~200-350 MiB（spec §4.4 预估）。

- [ ] **Step 3: 真实 sidecar 启动验证（替代假 sidecar）**

```bash
DSH_HOME=/tmp/dsh-m4-test ./src-tauri/resources/dsh/bin/node \
  ./src-tauri/resources/dsh/lib/bin.js --profile web --port 0 \
  --patch ./src-tauri/resources/dsh/patch/desktop.patch.yml &
sleep 8
curl --unix-socket /tmp/dsh-m4-test/run/dsh.sock \
  -H 'Content-Type: application/json' \
  -d '{"type":"server-request","rpcId":"m4-1","method":"host.describe","payload":{}}' \
  http://dsh/api/host.describe | head -c 200
kill %1
```

预期：有响应（无 key 时为业务错误 JSON，但连接与 HTTP 往返成功）。

> 注：desktop.patch.yml 中 `name: '@dsh-desktop/uds-carrier'`（R1 修正：scoped 名与包名一致，禁裸名）的解析——若 baseUrl 锚定 profile 目录而非 patch 目录（M1 spike 结论），需把插件也装入 `$DSH_HOME/profiles/web/node_modules/` 或按 M1 对策改用绝对路径物化 patch。

- [ ] **Step 4: 进程管理器接真实 sidecar（替换假 sidecar 参数；R1 修正：uds_path 从 $DSH_HOME 派生，非 resource_dir）**

`src-tauri/src/lib.rs`：
- **uds_path（R1 修正）**：从 `$DSH_HOME` 派生（缺省 `~/.dsh`），socket = `$DSH_HOME/run/dsh.sock`——与 M1 `selectSocketPath`/patch `udsPath` 一致；**不是** `resource_dir/dsh/run`（那在 .app bundle 内，与 carrier 实际监听路径不符）。
  ```rust
  fn default_socket_path() -> String {
      let home = std::env::var("DSH_HOME").unwrap_or_else(|_| {
          std::env::var("HOME").map(|h| format!("{h}/.dsh")).unwrap_or_else(|_| "/tmp/dsh-desktop".into())
      });
      format!("{home}/run/dsh.sock")
  }
  ```
- spawn 命令 = `bin/node lib/bin.js --profile web --port 0 --patch <abs>/patch/desktop.patch.yml`（`<abs>` = `app.path().resource_dir()?.join("dsh/patch/desktop.patch.yml")`），cwd = `$DSH_HOME`。
- **退出序列接线（R1 修正：显式任务，M2 验收 ②③ 在此验证）**：`RunEvent::ExitRequested` → `prevent_exit()` → 异步：① 取消重启定时器/退避 sleep ② SIGTERM(组)（`graceful_shutdown` 5s grace）③ SIGKILL(组) ④ unlink socket ⑤ `exit(0)`。
- **kill -9 退避节奏手动验证（M2 验收 ② 在此完成）**：启动 app → kill -9 sidecar → 观察日志退避节奏 1s→2s→4s→8s→停止；Cmd+Q → 观察 SIGTERM→5s→SIGKILL（M2 验收 ③ 在此完成）。

- [ ] **Step 5: 提交**

```bash
git add scripts/build-sidecar.sh src-tauri/tauri.conf.json .gitignore
git commit -m "feat(sidecar): node + package-dir layout build script"
```

---

### Task 6: 打包 + 签名 + 公证（arm64）

**Files:**
- Modify: `src-tauri/tauri.conf.json`（bundle 细节、identifier 定稿）
- Create: `scripts/notarize.sh`

**Interfaces:**
- Consumes: 签名证书（Apple Developer）、公证账号
- Produces: `.app`（arm64）+ 公证通过；签名/资源重排后 re-sign node 与 sidecar 二进制（spec §4.4）

- [ ] **Step 1: identifier 定稿 + bundle 配置**

`tauri.conf.json`：`identifier` 定稿（如 `com.dsh-desktop.app`——M4 定，影响 cache 路径/LaunchAgent/通知身份）；`bundle.macOS` 加 `signingIdentity` 相关（tauri 从环境读取，占位即可）。

- [ ] **Step 2: 全量构建**

```bash
cd src-tauri && cargo tauri build --bundles app 2>&1 | tail -10
```

预期：`target/release/bundle/macos/dsh-desktop.app` 生成。

- [ ] **Step 3: node/sidecar 二进制 re-sign（lipo 或资源重排后）**

```bash
APP=target/release/bundle/macos/dsh-desktop.app
codesign --force --deep --sign "Developer ID Application: ..." \
  "$APP/Contents/Resources/dsh/bin/node" 2>&1 | tail -3
codesign --verify --deep --strict "$APP" 2>&1 | tail -3
```

预期：verify 无错误（spec §4.4：lipo 合并或资源重排后需 re-sign node 与 sidecar）。

- [ ] **Step 4: notarize.sh（公证）**

`scripts/notarize.sh`：
```bash
#!/usr/bin/env bash
# 公证（spec §4.4）：hardened runtime + 公证；需要 Apple Developer 账号环境变量
set -euo pipefail
APP="${1:?usage: notarize.sh path/to/dsh-desktop.app}"
BUNDLE_ID="com.dsh-desktop.app"
# xcrun notarytool 需要 --apple-id/--team-id/--password（环境变量传入，勿落盘）
xcrun notarytool submit "$APP" --wait \
  --apple-id "${APPLE_ID:?}" --team-id "${TEAM_ID:?}" --password "${APPLE_APP_PASSWORD:?}" \
  2>&1 | tail -5
xcrun stapler staple "$APP"
echo "notarized: $APP"
```

- [ ] **Step 5: M4 验收 ①–⑧ 手动清单（逐个勾选）**

- [ ] 托盘显隐/退出
- [ ] 自启 LaunchAgent 注册注销（`~/Library/LaunchAgents/` 出现/消失）
- [ ] agent 完成通知（mock 或真实回合）
- [ ] 外链走系统浏览器且 target=_blank 不导航主窗口
- [ ] >10 MiB 附件/导出走既定路径
- [ ] 非白名单 invoke 被拒（前端调 `invoke('fs:read_file')` 应 reject）
- [ ] frontendDist 缺失弹错误对话框（删 dist 再启动）
- [ ] .app 签名+公证通过（arm64）；x64/universal 按架构定案

- [ ] **Step 6: 提交**

```bash
git add scripts/notarize.sh src-tauri/tauri.conf.json
git commit -m "feat(packaging): notarize script + identifier finalization"
```

---

## M4 完成检查（对照 spec §10 M4 验收）

- [ ] 托盘显隐/退出
- [ ] 自启 LaunchAgent 注册注销
- [ ] agent 完成通知
- [ ] 外链走系统浏览器且 target=_blank 不导航主窗口
- [ ] >10 MiB 附件与导出走既定路径（§4.6）
- [ ] 非白名单 invoke 被拒
- [ ] frontendDist 缺失弹错误对话框
- [ ] .app 签名+公证通过（arm64；x64/universal 按架构定案）
- [ ] identifier 定稿
