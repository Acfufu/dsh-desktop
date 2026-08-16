mod http_command;
#[cfg(test)]
mod bench_test;
mod streams;
mod process;
mod state_machine;

use http_command::dsh_http;
use streams::{dsh_cancel, dsh_close_stream, dsh_open_stream, StreamRegistry};
use std::sync::{Arc, Mutex};

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
        .manage(AppState {
            http_client,
            uds_path,
            registry: Arc::new(Mutex::new(StreamRegistry::new())),
        })
        .invoke_handler(tauri::generate_handler![dsh_http, dsh_open_stream, dsh_close_stream, dsh_cancel])
        .build(tauri::generate_context!())
        .expect("error while building tauri app")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
                // 异步关闭任务：取消重启定时器 → SIGTERM(组) → 5s → SIGKILL(组) → 清理
                tauri::async_runtime::spawn(async move {
                    // 占位：M4 接入真实进程管理器句柄；当前先记录日志
                    eprintln!("dsh-desktop: exit sequence triggered");
                    std::process::exit(0);
                });
            }
        });
}
