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
        // R3 修正：conf 加 bundle.icon 后 default_window_icon() 非 None；仍用 if let Some 防 panic
        .icon(app.default_window_icon().cloned().unwrap_or_else(|| {
            // 兜底：1×1 透明占位（不崩启动）
            tauri::image::Image::new_owned(vec![0; 4], 1, 1).unwrap()
        }))
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
                    crate::shutdown_sequence(app_handle).await; // R3 修正：Task 1 下方定义 stub，Task 5 Step 4 换完整版
                });
            }
        });
}

// R3 修正：Task 1 给可编译 stub（否则 Task 1 Step 4 cargo check 失败——引用未定义函数）
pub async fn shutdown_sequence(_app: tauri::AppHandle) {
    eprintln!("dsh-desktop: exit sequence triggered (stub)");
    std::process::exit(0);
}
```

- [ ] **Step 3: tauri.conf.json 追加（exitOnLastWindowClosed + 托盘图标；R3 修正：bundle.icon 必加——default_window_icon() 依赖它）**

```json
  "app": {
    "windows": [ ... 同 M2 ... ],
    "security": { ... 同 M2 ... },
    "trayIcon": { "iconPath": "icons/icon.png", "iconAsTemplate": true },
    "exitOnLastWindowClosed": false
  },
  "bundle": {
    "active": true,
    "targets": ["app"],
    "icon": ["icons/icon.png"],   // R3 修正：无 bundle.icon → default_window_icon() 恒 None
    "macOS": { "minimumSystemVersion": "12.0" },
    "resources": ["dsh/**"]
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
import { ServerRequest } from '@deepseek-ai/dsh-host-apiproxy/api'; // R3 修正：ServerRequest 不在根导出（实证在 /api 子路径）

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

`src-tauri/src/tempfiles.rs`（R2 修正：fs + PermissionsExt 显式导入）：
```rust
use std::path::{Path, PathBuf};
use std::fs; // R2 修正：生产代码用 fs::，必须显式导入
use std::os::unix::fs::PermissionsExt; // R2 修正：set_mode/from_mode 需要

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

// R2 修正：nanoid 从测试模块提到生产作用域（dsh_save_export/dsh_write_temp 生产代码调用它，
// 原定义在 #[cfg(test)] 内 → 非测试构建编译失败）
pub fn nanoid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{n:x}")
}

// R2 修正：删除测试模块内的重复 nanoid 定义（已提为生产 pub fn），sanitize 测试保留
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

- [ ] **Step 5: 上传/下载路径接线（前端；R2 修正：补 >10MiB 拖拽完整链路 + 下载不经 invoke bytes）**

**R2 修正：新增 Rust 命令 `dsh_import_dropped`（拖拽路径摄取——fs 插件被排除，需 Rust 侧命令；spec §4.6）：**
```rust
// tempfiles.rs 追加：
// 拖拽（onDragDropEvent）给原生路径 → Rust 直接读源文件（>10MiB 大文件）
#[tauri::command]
pub async fn dsh_import_dropped(path: String) -> Result<Vec<u8>, String> {
    let canon = std::path::Path::new(&path).canonicalize().map_err(|e| format!("canonicalize: {e}"))?;
    if !canon.is_file() { return Err("not a file".into()); }
    std::fs::read(&canon).map_err(|e| format!("read: {e}"))
}
```

- 窗口拖拽：WebviewWindowBuilder `.on_drag_drop_event(...)` → 拖拽路径调 `dsh_import_dropped` → 经 `dsh_http` 上传
- **上传 <10 MiB**：`File.arrayBuffer()` → `invoke('dsh_write_temp', ...)`
- **>10 MiB**：仅拖拽路径（`onDragDropEvent` → `dsh_import_dropped`）；选择器选中大文件 → 前端文件大小检查拒绝并提示
- **下载（R2 修正：session.export 大 ZIP 不经 invoke bytes 回传）**：前端请求导出 → `invoke('dsh_export_session', { sessionId })` → **Rust 侧流式落盘**（经 UDS 拉 session.export 流 → 写 ~/Downloads → 通知）；`dsh_save_export(bytes, file_name)` 仅保留给小文件/临时场景。
  > spec §6：150 MiB 响应约 23s 是 invoke 大 payload 代价——下载必须 Rust 侧流式落盘，禁止 bytes 经 invoke 回传前端。

- [ ] **Step 6: 编译 + 测试 + 提交**

```bash
cd src-tauri && cargo test 2>&1 | tail -5 && cargo check 2>&1 | tail -3
git add src-tauri/src/tempfiles.rs src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "feat(src-tauri): temp file discipline + export download"
```

---

### Task 4.5: 日志模块 + 错误对话框（spec §4.2 日志 + §6 对话框；R2 修正：新增显式任务）

**Files:**
- Create: `src-tauri/src/logging.rs`
- Create: `src-tauri/src/logging.rs`（单测：轮转、乱码解码）
- Create: `src-tauri/src/dialogs.rs`
- Modify: `src-tauri/src/process.rs`（spawn 接日志文件 + LC_ALL）
- Modify: `src-tauri/src/lib.rs`（RUST_LOG + panic hook + 对话框接线）

**Interfaces:**
- Consumes: `~/Library/Logs/dsh-desktop/` 目录
- Produces: `fn init_logging(app: &AppHandle) -> Result<PathBuf, String>`（1 MiB × 3 轮转）；`fn setup_panic_hook()`；`fn show_error_dialog(title, body)`（无 plugin-dialog——用 `tauri::window` 原生 alert 或最小内建消息框，§4.5 排除 dialog 插件，R2 修正：定载体为 `app.get_webview_window("main")?.eval("alert(...)")` 或 `rfd` 原生文件选择替代方案中取前者）

- [ ] **Step 1: 日志模块（轮转 + 解码）**

`src-tauri/src/logging.rs`：
```rust
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_BYTES: u64 = 1024 * 1024; // 1 MiB
const ROTATIONS: usize = 3;

/// sidecar stdout/stderr → ~/Library/Logs/dsh-desktop/sidecar.log（1 MiB × 3 轮转）
/// R2 修正：显式任务（spec §4.2 日志要求全无家）
pub fn init_sidecar_log(logs_dir: &Path) -> Result<File, String> {
    std::fs::create_dir_all(logs_dir).map_err(|e| e.to_string())?;
    let path = logs_dir.join("sidecar.log");
    rotate_if_needed(&path);
    OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())
}

fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() >= MAX_LOG_BYTES {
            for i in (1..ROTATIONS).rev() {
                let from = path.with_extension(format!("{}.{}.", "log", i)); // sidecar.log.<i>
                // 简化命名：sidecar.log.1 → sidecar.log.2 ...
                let _ = std::fs::rename(path.with_extension("log").with_file_name(format!("sidecar.log.{i}")), path.with_extension("log").with_file_name(format!("sidecar.log.{}", i + 1)));
            }
            let _ = std::fs::rename(path, path.with_file_name("sidecar.log.1"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_log_file() {
        let dir = std::env::temp_dir().join(format!("dsh-log-{}", std::process::id()));
        let f = init_sidecar_log(&dir).expect("init");
        assert!(dir.join("sidecar.log").exists());
        drop(f);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: spawn 接线（LC_ALL + 日志文件）**

`src-tauri/src/process.rs` 的 `spawn_sidecar` 追加：
```rust
cmd.env("LC_ALL", "en_US.UTF-8"); // spec §4.2：显式 locale 防乱码
// 日志解码：sidecar 输出在展示/落盘时 from_utf8_lossy（spec §4.2）——
// 落盘为原始字节 + 读取时 from_utf8_lossy 解码（诊断对话框 tail 20 行时应用）
```

- [ ] **Step 3: 对话框（错误对话框 + tail 20；R2 修正：载体 = 主窗口 eval alert，无 dialog 插件）**

`src-tauri/src/dialogs.rs`：
```rust
use tauri::{AppHandle, Manager};

/// 错误对话框（spec §6）：sidecar 启动失败 stderr tail 20 / frontendDist 缺失 / socket 权限异常。
/// R2 修正：§4.5 排除 dialog 插件 → 用主窗口原生 alert（eval）
pub fn show_error_dialog(app: &AppHandle, title: &str, body: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let js = format!("alert({:?})", format!("{title}\n\n{body}"));
        let _ = win.eval(&js);
    }
}
```

- [ ] **Step 4: lib.rs 接线（RUST_LOG + panic hook + 启动失败对话框；R3 修正：setup_panic_hook/tail20 此前只声明未实现——此处给完整定义）**

`src-tauri/src/logging.rs` 追加：
```rust
// R3 修正：panic hook + tail 读取（此前只出现在 Interfaces/注释，无实现）
pub fn setup_panic_hook(logs_dir: &Path) {
    let dir = logs_dir.to_path_buf();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("panic: {info}");
        let _ = std::fs::write(dir.join("dsh-desktop-panic.log"), &msg);
        eprintln!("{msg}");
    }));
}

/// 读日志尾部 N 行（诊断对话框；from_utf8_lossy 解码，spec §4.2）
pub fn tail_lines(path: &Path, n: usize) -> String {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            text.lines().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
        }
        Err(_) => String::new(),
    }
}
```

`src-tauri/src/lib.rs` run() 顶部（R3 修正：`?` 只用于 Result 返回函数——run() 内改为 `expect`/`unwrap_or`）：
```rust
std::env::set_var("RUST_LOG", "info"); // spec §4.2：App 自身日志
let logs_dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Logs/dsh-desktop");
logging::setup_panic_hook(&logs_dir);
let log_file = logging::init_sidecar_log(&logs_dir).expect("init sidecar log"); // run() 非 Result，用 expect
// first-starting 失败：let tail = logging::tail_lines(&sidecar_log_path, 20); dialogs::show_error_dialog(app, "dsh-desktop 启动失败", &tail);
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/logging.rs src-tauri/src/dialogs.rs src-tauri/src/process.rs src-tauri/src/lib.rs
git commit -m "feat(src-tauri): logging module + error dialogs"
```

---

### Task 4.75: ProcessManager 完整实现（R3 修正：ProcessManager/cancel_restart/take_child 只被引用从未定义；CancellationToken 依赖 tokio-util）

**Files:**
- Create: `src-tauri/src/process_manager.rs`
- Modify: `src-tauri/Cargo.toml`（tokio-util）
- Modify: `src-tauri/src/lib.rs`（manage ProcessManager；shutdown_sequence 换完整版）

**Interfaces:**
- Consumes: M2 `spawn_sidecar`/`graceful_shutdown`/`probe_socket`/`transition`/`RestartCounter`
- Produces: `pub struct ProcessManager`（state + counter + child + cancel token）+ `cancel_restart()`/`take_child()`/`start()`；`shutdown_sequence` 完整版（SIGTERM→5s→SIGKILL→unlink）

- [ ] **Step 1: Cargo.toml 加 tokio-util**

```toml
tokio-util = "0.7"
```

- [ ] **Step 2: ProcessManager 实现（R3 修正：给出可编译结构；watch/退避循环以 M4 手动验证为验收）**

`src-tauri/src/process_manager.rs`：
```rust
use crate::process::{graceful_shutdown, spawn_sidecar};
use crate::state_machine::{AppState2, RestartCounter};
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;
use tokio::process::Child;
use tokio_util::sync::CancellationToken;

pub struct ProcessManager {
    pub state: Mutex<AppState2>,
    pub counter: Mutex<RestartCounter>,
    pub child: Mutex<Option<Child>>,
    pub cancel: CancellationToken,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(AppState2::Stopped),
            counter: Mutex::new(RestartCounter::new()),
            child: Mutex::new(None),
            cancel: CancellationToken::new(),
        }
    }

    /// 退出序列 ①：取消重启定时器/退避
    pub async fn cancel_restart(&self) {
        self.cancel.cancel();
    }

    /// 退出序列 ②③：取走 child 做 SIGTERM→5s→SIGKILL
    pub async fn take_child(&self) -> Option<Child> {
        self.child.lock().ok()?.take()
    }

    /// 启动 sidecar + watch 循环（watch 循环骨架：退避复用 M2 transition/on_exit；完整节奏 M4 手动验证）
    pub async fn start(&self, _app: &AppHandle) -> Result<(), String> {
        Ok(()) // 骨架：M4 Task 5 Step 4 接真实 spawn_sidecar 参数
    }
}
```

- [ ] **Step 3: lib.rs 接线 + shutdown_sequence 完整版**

```rust
// run() 内：.manage(ProcessManager::new())
// shutdown_sequence 替换 Task 1 stub：
// pub async fn shutdown_sequence(app: tauri::AppHandle) {
//     if let Some(mgr) = app.try_state::<ProcessManager>() {
//         mgr.cancel_restart().await;
//         if let Some(mut child) = mgr.take_child().await {
//             graceful_shutdown(&mut child, Duration::from_secs(5)).await;
//         }
//     }
//     let _ = std::fs::remove_file(crate::process::default_socket_path());
//     std::process::exit(0);
// }
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/process_manager.rs src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat(src-tauri): ProcessManager with cancel-token restart control"
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
- **`shutdown_sequence` 完整定义（R2 修正：M4 Task 1 引用它，此处必须给出可编译函数）**：
  ```rust
  // lib.rs：
  pub async fn shutdown_sequence(app: tauri::AppHandle) {
      // ① 取消重启定时器/退避 sleep（进程管理器持有的 CancellationToken）
      if let Some(mgr) = app.try_state::<crate::process::ProcessManager>() {
          mgr.cancel_restart().await;
          // ② SIGTERM(组)：graceful_shutdown(child, 5s)
          if let Some(child) = mgr.take_child().await {
              crate::process::graceful_shutdown(&mut child, std::time::Duration::from_secs(5)).await;
          }
      }
      // ④ unlink socket + 清临时文件
      let sock = default_socket_path();
      let _ = std::fs::remove_file(&sock);
      // ⑤ exit(0)
      std::process::exit(0);
  }
  ```
  > 注：`ProcessManager`/`cancel_restart`/`take_child` 为进程管理器对外接口（本 Task 定义，M2 状态机实现复用）；以编译通过为验收。
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
