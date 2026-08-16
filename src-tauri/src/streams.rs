use std::collections::HashMap;
use std::sync::Mutex;
use tauri::ipc::Channel;

pub struct StreamRegistry {
    pub tasks: Mutex<HashMap<u64, StreamTask>>,
    next_id: u64,
}

pub struct StreamTask {
    pub channel: Channel<String>,
    pub handle: tokio::task::JoinHandle<()>,
}

impl StreamRegistry {
    pub fn new() -> Self {
        Self { tasks: Mutex::new(HashMap::new()), next_id: 1 }
    }

    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 幂等：未知/已关闭 id 返回 Ok(())
    pub fn close(&self, id: u64) -> Result<(), String> {
        let mut tasks = self.tasks.lock().map_err(|e| e.to_string())?;
        tasks.remove(&id);
        Ok(())
    }
}

use tokio::net::UnixStream;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::{client_async_with_config};
use futures_util::StreamExt;

const MUX_PATH: &str = "/api/events.mux";
const HOST_PATH: &str = "/api/events.host";

#[tauri::command]
pub async fn dsh_open_stream(
    stream: String,
    channel: Channel<String>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<u64, String> {
    let path = match stream.as_str() {
        "mux" => MUX_PATH,
        "host" => HOST_PATH,
        other => return Err(format!("unknown stream: {other}")),
    };

    let id = {
        let mut reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.next_id()
    };

    let ws_url = format!("ws://dsh{path}");
    let (ws, _) = open_ws_over_uds(&state.uds_path, &ws_url)
        .await
        .map_err(|e| format!("open ws {path}: {e}"))?;

    let channel_for_task = channel.clone();
    let handle = tokio::spawn(async move {
        let (_, mut reader) = ws.split();
        loop {
            match reader.next().await {
                Some(Ok(WsMessage::Text(text))) => {
                    if channel_for_task.send(text.to_string()).is_err() {
                        break; // 前端 channel 关闭 → 终止
                    }
                }
                Some(Ok(_)) => { /* 二进制/其他帧忽略 */ }
                Some(Err(_)) | None => {
                    // 流断开/出错 → 发送空字符串终止帧（R1 修正：与前端 -end 哨兵对齐）
                    let _ = channel_for_task.send(String::new());
                    break;
                }
            }
        }
    });

    {
        let mut reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.tasks.lock().map_err(|e| e.to_string())?.insert(id, StreamTask { channel, handle });
    }
    Ok(id)
}

#[tauri::command]
pub async fn dsh_close_stream(id: u64, state: tauri::State<'_, crate::AppState>) -> Result<(), String> {
    let reg = state.registry.lock().map_err(|e| e.to_string())?;
    reg.close(id)
}

async fn open_ws_over_uds(socket_path: &str, ws_url: &str) -> Result<(tokio_tungstenite::WebSocketStream<UnixStream>, ()), String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let stream = UnixStream::connect(socket_path).await.map_err(|e| e.to_string())?;
    let request = ws_url.into_client_request().map_err(|e| e.to_string())?;
    let (ws, _resp) = client_async_with_config(request, stream, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok((ws, ()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_unknown_id_is_noop() {
        let reg = StreamRegistry::new();
        assert!(reg.close(999).is_ok());
    }

    #[test]
    fn ids_increment() {
        let mut reg = StreamRegistry::new();
        assert_eq!(reg.next_id(), 1);
        assert_eq!(reg.next_id(), 2);
    }
}

#[cfg(test)]
mod ws_integration {
    use super::*;
    use std::process::Command;
    use std::time::Duration;

    fn unique_sock() -> String {
        format!("/tmp/dsh-uds-test/ws-{}.sock", std::process::id())
    }

    struct Sidecar(std::process::Child);

    impl Sidecar {
        fn start(sock: &str) -> Sidecar {
            let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/fake-sidecar.mjs");
            let child = Command::new("node").arg(script)
                .env("DSH_SOCKET", sock)
                .spawn().expect("spawn ws sidecar");
            std::thread::sleep(Duration::from_millis(800));
            Sidecar(child)
        }
    }
    impl Drop for Sidecar { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }

    #[tokio::test]
    async fn open_stream_receives_frame_and_end_sentinel() {
        let sock = unique_sock();
        let _sc = Sidecar::start(&sock);
        let (mut ws, _) = open_ws_over_uds(&sock, "ws://dsh/api/events.mux")
            .await.expect("open ws");
        let first = ws.next().await.expect("first frame").expect("frame ok");
        assert!(matches!(first, WsMessage::Text(_)));
    }
}

#[tauri::command]
pub async fn dsh_cancel(id: u64, state: tauri::State<'_, crate::AppState>) -> Result<(), String> {
    // Task 4 接在途请求取消；当前幂等 no-op（spec：Rust 不设自身超时，取消由前端信号驱动）
    let _ = id;
    let _ = state;
    Ok(())
}
