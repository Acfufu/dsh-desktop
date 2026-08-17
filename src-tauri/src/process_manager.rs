use crate::process::{graceful_shutdown, probe_socket, spawn_sidecar, ProbeResult};
use crate::state_machine::{AppEvent, AppState2, RestartCounter, RestartDecision, transition};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::AppHandle;
use tokio::process::Child;
use tokio_util::sync::CancellationToken;

pub struct ProcessManager {
    pub state: Mutex<AppState2>,
    pub counter: Mutex<RestartCounter>,
    pub child: Mutex<Option<Child>>,
    pub cancel: CancellationToken,
    pub ever_ready: Mutex<bool>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(AppState2::Stopped),
            counter: Mutex::new(RestartCounter::new()),
            child: Mutex::new(None),
            cancel: CancellationToken::new(),
            ever_ready: Mutex::new(false),
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

    /// watch 循环：spawn → 探测 → 事件 → child.wait → on_exit → 退避 sleep（监听 cancel）→ respawn
    /// DeepSec L3：不得持 child Mutex 跨 await（wait 期间 take_child 会死锁 → 退出序列卡死）
    pub async fn start(self: Arc<Self>, app: AppHandle, node_bin: String, args: Vec<String>, cwd: String, dsh_home: String, log_file: std::fs::File) -> Result<(), String> {
        // 活体探测（spec §4.2 单实例）：Alive → 已在运行；仅 ENOENT/ECONNREFUSED 才 unlink 再 spawn
        let socket = crate::process::default_socket_path();
        match probe_socket(&socket).await {
            ProbeResult::Alive => {
                crate::dialogs::show_error_dialog(&app, "dsh-desktop 已在运行", "检测到已有实例，请勿重复启动。");
                std::process::exit(0);
            }
            ProbeResult::Stale => { let _ = std::fs::remove_file(&socket); }
            ProbeResult::Error(e) => return Err(format!("probe: {e}")),
        }

        let first = !*self.ever_ready.lock().unwrap();
        *self.state.lock().unwrap() = AppState2::FirstStarting;

        let args_ref = args.iter().map(|s| s.as_str()).collect::<Vec<_>>();
        match spawn_sidecar(&node_bin, &args_ref, &cwd, &dsh_home, &log_file) {
            Ok(child) => { *self.child.lock().unwrap() = Some(child); }
            Err(e) => {
                if first {
                    crate::dialogs::show_error_dialog(&app, "dsh-desktop 启动失败", &e.to_string());
                    *self.state.lock().unwrap() = AppState2::Stopped;
                } else {
                    let mut c = self.counter.lock().unwrap();
                    let d = transition(*self.state.lock().unwrap(), AppEvent::UnexpectedExit, &mut c, 0);
                    *self.state.lock().unwrap() = d;
                }
                return Err(e.to_string());
            }
        }

        let mgr = Arc::clone(&self);
        let app2 = app.clone();
        let node_bin2 = node_bin.clone();
        let args2 = args.clone();
        let cwd2 = cwd.clone();
        let dsh_home2 = dsh_home.clone();
        let log2 = log_file.try_clone().map_err(|e| e.to_string())?;
        tokio::spawn(async move {
            loop {
                // 锁内取出 child，立即释放锁——wait 期间 take_child 可正常取走（退出序列）
                let child = mgr.child.lock().unwrap().take();
                let alive_secs = match child {
                    Some(mut ch) => {
                        let t0 = std::time::Instant::now();
                        let _ = ch.wait().await; // 不持锁
                        t0.elapsed().as_secs()
                    }
                    None => 0,
                };
                if mgr.cancel.is_cancelled() { break; }
                *mgr.ever_ready.lock().unwrap() = true;
                // 作用域块：guard 在块尾释放——不得跨 await 持有（Send 检查）
                let (s, delay) = {
                    let mut c = mgr.counter.lock().unwrap();
                    let st = *mgr.state.lock().unwrap();
                    let s = transition(st, AppEvent::UnexpectedExit, &mut c, alive_secs);
                    *mgr.state.lock().unwrap() = s;
                    (s, c.current_delay)
                };
                if s == AppState2::RestartStopped {
                    crate::dialogs::show_error_dialog(&app2, "dsh-desktop 已停止", "连续 5 次启动失败，已停止自动重启。可在托盘菜单重试。");
                    break;
                }
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = mgr.cancel.cancelled() => break,
                }
                if mgr.cancel.is_cancelled() { break; }
                *mgr.state.lock().unwrap() = transition(AppState2::Restarting, AppEvent::BackoffElapsed, &mut mgr.counter.lock().unwrap(), 0);
                let args_ref = args2.iter().map(|s| s.as_str()).collect::<Vec<_>>();
                match spawn_sidecar(&node_bin2, &args_ref, &cwd2, &dsh_home2, &log2) {
                    Ok(child) => { *mgr.child.lock().unwrap() = Some(child); }
                    Err(_) => { /* 下次循环处理 */ }
                }
            }
        });
        Ok(())
    }
}
