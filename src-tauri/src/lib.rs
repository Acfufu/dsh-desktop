mod http_command;
#[cfg(test)]
mod bench_test;
mod streams;
mod process;
mod state_machine;
mod tray;

use http_command::dsh_http;
use streams::{dsh_cancel, dsh_close_stream, dsh_open_stream, StreamRegistry};
use std::sync::{Arc, Mutex};
use tauri::Manager;

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
    let uds_path = std::env::var("DSH_SOCKET").unwrap_or_else(|_| http_command::UDS_PATH.to_string());
    let http_client = reqwest::ClientBuilder::new()
        .unix_socket(uds_path.as_str())
        .redirect(reqwest::redirect::Policy::none()) // DeepSec L3：禁用重定向——防侧车响应驱动 reqwest 到非 /api 路径
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

// Task 5 Step 4 替换为完整退出序列（SIGTERM→5s→SIGKILL→unlink）
pub async fn shutdown_sequence(_app: tauri::AppHandle) {
    eprintln!("dsh-desktop: exit sequence triggered (stub)");
    std::process::exit(0);
}
