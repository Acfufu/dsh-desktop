mod http_command;
mod navigation;
mod logging;
mod dialogs;
mod tempfiles;

#[cfg(test)]
mod bench_test;
mod streams;
mod process;
mod process_manager;
mod state_machine;
mod tray;

use http_command::dsh_http;
use process::{default_socket_path, graceful_shutdown};
use process_manager::ProcessManager;
use streams::{dsh_cancel, dsh_close_stream, dsh_open_stream, StreamRegistry};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;
use tauri::webview::WebviewWindowBuilder;

// R5 修正：AppState 唯一真源（Task 2 起）；http_command.rs 只 use crate::AppState
pub struct AppState {
    pub http_client: reqwest::Client,
    pub uds_path: String,
    pub registry: Arc<Mutex<StreamRegistry>>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            uds_path: self.uds_path.clone(),
            registry: Arc::clone(&self.registry),
        }
    }
}

pub fn run() {
    std::env::set_var("RUST_LOG", "info"); // spec §4.2：App 自身日志
    let logs_dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Logs/dsh-desktop");
    logging::setup_panic_hook(&logs_dir);
    let uds_path = std::env::var("DSH_SOCKET").unwrap_or_else(|_| http_command::UDS_PATH.to_string());
    let http_client = reqwest::ClientBuilder::new()
        .unix_socket(uds_path.as_str())
        .redirect(reqwest::redirect::Policy::none()) // DeepSec L3：禁用重定向——防侧车响应驱动 reqwest 到非 /api 路径
        .build()
        .expect("build reqwest client with unix socket");

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
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
        .manage(Arc::new(ProcessManager::new()))
        .setup(move |app| {
            let logs_dir = logs_dir.clone();
            // conf 窗口无法挂 on_navigation → 主窗口由 builder 创建（R5：conf app.windows 已删）
            let win = WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
                .title("dsh-desktop")
                .inner_size(1200.0, 800.0)
                .on_navigation(|url| navigation::allowed_navigation(url.as_str(), cfg!(debug_assertions)))
                .build()?;
            let _ = win;
            tray::build_tray(app)?;

            // DeepSec L3：$DSH_HOME 与 .env 权限收紧（否则 DEEPSEEK_API_KEY 多用户可读）
            let dsh_home = std::env::var("DSH_HOME").unwrap_or_else(|_| {
                std::env::var("HOME").map(|h| format!("{h}/.dsh")).unwrap_or_default()
            });
            if !dsh_home.is_empty() {
                let _ = std::fs::create_dir_all(&dsh_home);
                std::fs::set_permissions(&dsh_home, std::fs::Permissions::from_mode(0o700)).ok();
                let env_file = std::path::Path::new(&dsh_home).join(".env");
                if env_file.exists() {
                    std::fs::set_permissions(&env_file, std::fs::Permissions::from_mode(0o600)).ok();
                }
            }
            // age 清扫孤儿临时文件（spec §4.6）
            if let Ok(cache) = app.path().app_cache_dir() {
                tempfiles::age_sweep(&cache, Duration::from_secs(24 * 3600));
            }

            // 启动 sidecar（dev 下 DSH_HOME 未设时用假路径——真实 sidecar 由 M4 Task 5 打包后经 resource_dir 提供）
            let pm = app.state::<Arc<ProcessManager>>();
            let dsh_dir = std::env::var("DSH_SIDECAR_DIR").unwrap_or_default();
            let (node_bin, args, cwd) = if !dsh_dir.is_empty() {
                let abs_patch = format!("{dsh_dir}/patch/desktop.patch.yml");
                (
                    format!("{dsh_dir}/bin/node"),
                    // 注：不加 --port（app 级参数，放在 launcher 旗标区会吞掉后续 --patch；webserver 已禁，端口无意义）
                    vec!["lib/bin.js".into(), "--profile".into(), "web".into(), "--patch".into(), abs_patch],
                    dsh_dir.clone(),
                )
            } else {
                // dev 占位：不 spawn（M3 e2e 手动起 sidecar）；ProcessManager 仍注册
                ("".into(), vec![], "".into())
            };
            if !node_bin.is_empty() {
                let log_file = logging::init_sidecar_log(&logs_dir).expect("init sidecar log");
                let pm2 = Arc::clone(&pm);
                let app2 = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = pm2.start(app2, node_bin, args, cwd, log_file).await;
                });
            }
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
        .invoke_handler(tauri::generate_handler![
            dsh_http, dsh_open_stream, dsh_close_stream, dsh_cancel,
            tempfiles::dsh_save_export, tempfiles::dsh_write_temp,
            tempfiles::dsh_import_dropped, tempfiles::dsh_export_session,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
                let handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    crate::shutdown_sequence(handle).await; // Task 5 Step 4 换完整版
                });
            }
        });
}

// 完整退出序列（spec §4.2）：取消重启定时器 → SIGTERM(组) → 5s → SIGKILL(组) → unlink socket
pub async fn shutdown_sequence(app: tauri::AppHandle) {
    if let Some(mgr) = app.try_state::<Arc<ProcessManager>>() {
        mgr.cancel_restart().await;
        if let Some(mut child) = mgr.take_child().await {
            graceful_shutdown(&mut child, Duration::from_secs(5)).await;
        }
    }
    let _ = std::fs::remove_file(default_socket_path());
    std::process::exit(0);
}
