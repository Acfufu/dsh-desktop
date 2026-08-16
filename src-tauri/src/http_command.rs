use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// 输入校验（纵深防御，非信任边界——spec §4.2）：method ∈ {POST,GET,HEAD}；
/// path 以 /api/ 开头且不含控制字符/空白。
pub fn validate_request(method: &str, path: &str) -> Result<(), String> {
    if !matches!(method, "POST" | "GET" | "HEAD") {
        return Err(format!("unsupported method: {method}"));
    }
    if !path.starts_with("/api/") {
        return Err(format!("path must start with /api/: {path}"));
    }
    if path.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(format!("path contains control/whitespace chars: {path}"));
    }
    Ok(())
}

use tauri::State;
use crate::AppState; // R5 修正：唯一真源在 lib.rs

pub const UDS_PATH: &str = "/tmp/dsh-uds-test/dsh.sock"; // 假 sidecar 路径（Task 4 换成进程管理器提供）

// R5 修正：核心逻辑抽成纯函数（可测，绕开 tauri State 注入）；命令薄包装
pub async fn dsh_http_impl(
    state: AppState,
    method: String,
    path: String,
    body: Option<Vec<u8>>,
) -> Result<HttpResponse, String> {
    validate_request(&method, &path)?;
    // DeepSec L3：body 大小上限（与 DEFAULT_MAX_REQUEST_BODY_BYTES 对齐）——XSS 可 invoke 超大 body 造成 OOM DoS
    if let Some(b) = &body {
        if b.len() > 160 * 1024 * 1024 {
            return Err("body exceeds 160 MiB".into());
        }
    }

    // POST 固定 JSON content-type（spec §4.2）；无 headers 参数
    let mut builder = state
        .http_client
        .request(reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?, &format!("http://dsh{path}"))
        .header("Host", "dsh");

    let body_bytes = body.clone().unwrap_or_default();
    if method == "POST" {
        builder = builder.header("Content-Type", "application/json").body(body_bytes);
    } else if method == "GET" || method == "HEAD" {
        builder = builder.body(body_bytes);
    }

    // 不设自身超时（spec §4.2）；R3 修正：connect 错误重建 client 重试一次（spec §4.2）
    match builder.send().await {
        Ok(resp) => resp_to_http(resp).await,
        Err(e) if e.is_connect() => {
            let new_client = reqwest::ClientBuilder::new()
                .unix_socket(state.uds_path.as_str())
                .build()
                .map_err(|e| format!("rebuild client: {e}"))?;
            let mut retry = new_client
                .request(reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?, &format!("http://dsh{path}"))
                .header("Host", "dsh");
            let body2 = body.unwrap_or_default();
            if method == "POST" {
                retry = retry.header("Content-Type", "application/json").body(body2);
            } else {
                retry = retry.body(body2);
            }
            resp_to_http(retry.send().await.map_err(|e| format!("transport after rebuild: {e}"))?).await
        }
        Err(e) => Err(format!("transport: {e}")),
    }
}

async fn resp_to_http(resp: reqwest::Response) -> Result<HttpResponse, String> {
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.to_string(), s.to_string())))
        .collect::<HashMap<_, _>>();
    let body = resp.bytes().await.map_err(|e| format!("read: {e}"))?.to_vec();
    Ok(HttpResponse { status, headers, body })
}

#[tauri::command]
pub async fn dsh_http(
    state: State<'_, crate::AppState>,
    method: String,
    path: String,
    body: Option<Vec<u8>>,
) -> Result<HttpResponse, String> {
    dsh_http_impl(state.inner().clone(), method, path, body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_whitelisted_methods() {
        assert!(validate_request("DELETE", "/api/x").is_err());
        assert!(validate_request("PATCH", "/api/x").is_err());
    }

    #[test]
    fn accepts_whitelisted_methods() {
        assert!(validate_request("POST", "/api/x").is_ok());
        assert!(validate_request("GET", "/api/x").is_ok());
        assert!(validate_request("HEAD", "/api/x").is_ok());
    }

    #[test]
    fn rejects_non_api_paths() {
        assert!(validate_request("POST", "/api2/x").is_err());
        assert!(validate_request("POST", "/x").is_err());
    }

    #[test]
    fn rejects_control_and_whitespace() {
        assert!(validate_request("POST", "/api/a b").is_err());
        assert!(validate_request("POST", "/api/a\tb").is_err());
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use std::process::{Child, Command};
    use std::time::Duration;
    use std::sync::{Arc, Mutex};

    fn unique_sock(tag: &str) -> String {
        format!("/tmp/dsh-uds-test/{}-{}.sock", tag, std::process::id())
    }

    struct Sidecar(Child);

    impl Sidecar {
        fn start(sock: &str) -> Sidecar {
            // R2 修正：用 CARGO_MANIFEST_DIR 锚定绝对路径（cargo test cwd = src-tauri，相对路径找不到根 scripts/）
            let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/fake-sidecar.mjs");
            let child = Command::new("node")
                .arg(script)
                .env("DSH_SOCKET", sock)
                .spawn()
                .expect("spawn fake sidecar");
            std::thread::sleep(Duration::from_millis(800)); // 等 socket 就绪
            Sidecar(child)
        }
    }

    impl Drop for Sidecar {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[tokio::test]
    async fn uplink_roundtrip_hits_fake_sidecar() {
        let sock = unique_sock("uplink");
        let _sc = Sidecar::start(&sock);
        let client = reqwest::ClientBuilder::new()
            .unix_socket(sock.as_str())
            .build()
            .unwrap();
        let state = crate::AppState {
            http_client: client,
            uds_path: sock,
            registry: std::sync::Arc::new(std::sync::Mutex::new(crate::streams::StreamRegistry::new())),
        };

        let resp = dsh_http_impl(state, "POST".into(), "/api/host.describe".into(), Some(br#"{}"#.to_vec()))
            .await
            .expect("call succeeds");
        assert_eq!(resp.status, 200);
        let json: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(json["method"], "host.describe");
    }
}
