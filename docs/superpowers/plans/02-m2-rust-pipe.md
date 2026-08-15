# M2 — Rust 哑管道 + 进程管理 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> 共享事实底座：先读 `docs/superpowers/plans/00-verified-facts-and-corrections.md`（版本 pin、符号、修正全部以它为准）。

**Goal:** 建 `src-tauri/`（tauri v2 crate），实现三类核心能力并全部有单测：(1) `dsh_http` uplink——reqwest unix_socket 对 UDS 发 HTTP/1.1，返回原始字节；(2) `dsh_open_stream`/`dsh_close_stream`/`dsh_cancel` downlink——tokio-tungstenite over UnixStream，经 `tauri::ipc::Channel<String>` 按序投递；(3) 进程管理器——spawn sidecar（setpgid 独立进程组）、退避重启（存活<30s 计数、≥30s 重置、5 次停止）、单实例/活体探测三分支、退出序列（SIGTERM→5s→SIGKILL→清理）。手动验证：假 sidecar 下 kill -9 观察退避节奏。

**Architecture:** Rust 只做 socket 搬运（不解析业务帧）。`dsh_http` = 单命令：reqwest `ClientBuilder::unix_socket`（0.12.28，`#[cfg(unix)]` 目标门控，**无需 feature flag**）；downlink 三命令共享一个 `Mutex<HashMap<u64, StreamTask>>` 注册表；进程管理独立模块 `process.rs` + `state_machine.rs`（状态机先写单测）。sidecar 二进制在 M4 之前用假 sidecar（`scripts/fake-sidecar.mjs`）驱动单测与手动验证。

**Tech Stack:** Rust 1.97.1（MSRV 1.77.2 满足）、tauri 2.11.5、tauri-build 2.6.3、tokio 1.53、reqwest 0.12.28、tokio-tungstenite 0.29、serde/serde_json 1。版本实证见 facts 文档 §6。

## Global Constraints

- 执行者 = deepseek-v4-flash 级别：**每步完整代码**，禁止「参考上一步」；命令与预期输出逐字给出。
- 禁止 `as any` 等价物（Rust 无）；禁止 `unwrap()` 泄漏进生产路径——测试可 `unwrap()`，生产路径必须 `?`/`map_err`。
- **Rust 不设自身超时**（unary 超时在前端调用点，spec §4.2）；`dsh_http` 对 POST 固定 `Content-Type: application/json`。
- `dsh_open_stream`：只在前端开流请求时建立；任一流断开 → **发送空字符串终止帧 `""` 后终止 channel**（R1 修正：与 M3 的 `-end` 哨兵对齐——M3 openDownlink 以 `text === ''` 判终，Rust 必须在流结束/出错时 send("")，不能只 drop channel）；`dsh_close_stream` 对未知 id **幂等 no-op**。
- 退出序列：取消重启定时器（期间禁止 spawn）→ SIGTERM(组) → 5s → SIGKILL(组) → unlink socket + 清临时文件 → exit。
- **进程组（R1 修正）**：spawn 用 `std::os::unix::process::CommandExt::process_group(0)`（安全 API，无 pre_exec/unsafe），使 sidecar 成为独立进程组组长；`graceful_shutdown` 的 `kill(-pid, ...)` 才安全（否则 -pid 指向 tauri 自身进程组）。
- tauri-cli 本机未装：`cargo install tauri-cli --locked --version 2.11.4`（或 npm i -D @tauri-apps/cli@2.11.4，二选一，M2 先用 cargo）。
- 测试：`cargo test` + tokio；`tauri::test` 若 mock 运行时不可达 ACL 则退化（capability 内容断言 + e2e 补，spec §7）。

---

### Task 1: src-tauri 脚手架 + 版本锁定

**Files:**
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/capabilities/desktop.json`
- Create: `src-tauri/icons/icon.png`（R2 修正：Task 1 即生成占位 PNG——tauri.conf.json trayIcon 引用它，M2「可空」会导致 cargo check 失败）

`src-tauri/icons/icon.png` 生成（R3 修正：printf 手拼 PNG 损坏——实证 zlib CRC 失败，Rust image crate 必解码失败；改用 base64 已验证 1×1 PNG）：
```bash
mkdir -p src-tauri/icons
printf 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==' | base64 -d > src-tauri/icons/icon.png
file src-tauri/icons/icon.png   # 预期: PNG image data, 1 x 1
```

**Interfaces:**
- Consumes: 无（脚手架）
- Produces: 可 `cargo check` 的空 tauri 应用；`run()`（lib.rs）供 main.rs 调用

- [ ] **Step 1: Cargo.toml（版本全部实证，facts §6）**

`src-tauri/Cargo.toml`：
```toml
[package]
name = "dsh-desktop"
version = "0.1.0"
description = "DeepSeek Harness desktop shell"
edition = "2021"
rust-version = "1.77.2"

[lib]
name = "dsh_desktop_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2.6", features = [] }

[dependencies]
tauri = { version = "2.11", features = [] }
tauri-plugin-single-instance = "2.4"
tauri-plugin-autostart = "2.5"
tauri-plugin-notification = "2.3"
tauri-plugin-opener = "2.5"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1.53", features = ["full"] }
reqwest = { version = "0.12.28", default-features = false, features = ["rustls-tls", "json"] }
tokio-tungstenite = "0.29.0"
futures-util = "0.3"
url = "2"
libc = "0.2"
```

- [ ] **Step 2: build.rs + tauri.conf.json（§4.7 配置要点）**

`src-tauri/build.rs`：
```rust
fn main() {
    tauri_build::build()
}
```

`src-tauri/tauri.conf.json`：
```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "dsh-desktop",
  "version": "0.1.0",
  "identifier": "com.dsh-desktop.app",
  "build": {
    "beforeDevCommand": "",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "",
    "frontendDist": "../frontend/dist"
  },
  // R2 修正：frontendDist 在 M2 时不存在 → 需建空占位目录（M3 构建后真实产物覆盖）
  "app": {
    "windows": [
      {
        "title": "dsh-desktop",
        "width": 1200,
        "height": 800
      }
    ],
    "security": {
      "devtools": false,
      "assetProtocol": {
        "enable": true,
        "scope": ["$APPCACHE/**"]
      }
    },
    "trayIcon": { "iconPath": "icons/icon.png" }
  },
  "bundle": {
    "active": true,
    "targets": ["app"],
    "macOS": {
      "minimumSystemVersion": "12.0"
    },
    "resources": ["dsh/**"]
  }
}
```

- [ ] **Step 3: main.rs + lib.rs 空壳**

`src-tauri/src/main.rs`：
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dsh_desktop_lib::run()
}
```

`src-tauri/src/lib.rs`（占位，后续 Task 填充）：
```rust
pub fn run() {
    println!("dsh-desktop placeholder");
}
```

- [ ] **Step 4: capabilities 骨架（§4.5 最小集，M2 先只放 transport 命令占位）**

`src-tauri/capabilities/desktop.json`：
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "desktop-capability",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "notification:default",
    "autostart:allow-enable",
    "autostart:allow-disable",
    "autostart:allow-is-enabled",
    "opener:default"
  ]
}
```

- [ ] **Step 5: 安装 tauri-cli + cargo check（R2 修正：先建 frontend/dist 空占位）**

```bash
mkdir -p ../frontend/dist   # frontendDist 占位（M2 尚无前端；M3 构建后真实产物覆盖）
cargo install tauri-cli --locked --version 2.11.4
cd src-tauri && cargo check 2>&1 | tail -5
```

预期：`Finished dev [unoptimized + debuginfo]`（首次构建数分钟，`tail -5` 看到 Finished 即可）。若仍报 frontendDist not found，确认 tauri.conf.json 的 `frontendDist` 相对 src-tauri 解析（`../frontend/dist`）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri
git commit -m "feat(src-tauri): scaffold tauri v2 crate with pinned deps"
```

---

### Task 2: dsh_http uplink（reqwest unix_socket）

**Files:**
- Create: `src-tauri/src/http_command.rs`
- Create: `src-tauri/src/http_command.rs`（单测同文件或 `tests/`——选择同文件 `#[cfg(test)]`）
- Modify: `src-tauri/src/lib.rs`（注册命令）

**Interfaces:**
- Consumes: `reqwest::ClientBuilder::unix_socket`（0.12.28，目标门控）；UDS 路径由进程管理器提供（Task 4 前用常量 `UDS_PATH`）
- Produces: `#[tauri::command] dsh_http(method: String, path: String, body: Option<Vec<u8>>) -> Result<HttpResponse, String>`；`HttpResponse { status: u16, headers: HashMap<String,String>, body: Vec<u8> }`（derive Serialize/Deserialize）

- [ ] **Step 1: 写失败测试（输入校验 + 响应形状）**

`src-tauri/src/http_command.rs`：
```rust
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
```

- [ ] **Step 2: 运行确认失败**

```bash
cd src-tauri && cargo test validate_request 2>&1 | tail -5
```

预期：PASS（纯函数测试本应直接过；此步确认编译与测试框架就绪）。若已过，继续 Step 3。

- [ ] **Step 3: 实现 dsh_http_impl 纯函数 + AppState 定义 + 薄命令（R3 修正：AppState + StreamRegistry 完整代码在此落位，Task 2 不再前引用 Task 3；`dsh_http_impl` 给出完整实现）**

`src-tauri/src/http_command.rs` 追加（同时定义 AppState 与 StreamRegistry 的最小版——streams 注册表完整实现在 Task 3，此处先给可编译骨架）：
```rust
use tauri::State;
use std::sync::{Arc, Mutex};

pub const UDS_PATH: &str = "/tmp/dsh-uds-test/dsh.sock"; // 假 sidecar 路径（Task 4 换成进程管理器提供）

// R3 修正：AppState 定义在此（Task 2），Task 3/4 的 lib.rs 复用
pub struct AppState {
    pub http_client: reqwest::Client,
    pub uds_path: String,
    pub registry: Arc<Mutex<crate::streams::StreamRegistry>>,
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

// R3 修正：核心逻辑抽成纯函数（可测，绕开 tauri State 注入）；命令薄包装
pub async fn dsh_http_impl(
    state: AppState,
    method: String,
    path: String,
    body: Option<Vec<u8>>,
) -> Result<HttpResponse, String> {
    validate_request(&method, &path)?;

    // POST 固定 JSON content-type（spec §4.2）；无 headers 参数
    let mut builder = state
        .http_client
        .request(reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?, &format!("http://dsh{path}"))
        .header("Host", "dsh");

    let body_bytes = body.unwrap_or_default();
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
                .unix_socket(&state.uds_path)
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
```

> R3 修正：`crate::streams::StreamRegistry` 的**最小可编译骨架**定义于 `src-tauri/src/streams.rs`（Task 3 补全）：
```rust
// src-tauri/src/streams.rs（Task 2 先建骨架，Task 3 补 StreamTask/WS 逻辑）
use std::collections::HashMap;
use std::sync::Mutex;

pub struct StreamRegistry {
    pub tasks: Mutex<HashMap<u64, ()>>,
    next_id: u64,
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
}
```

- [ ] **Step 4: 假 sidecar（可编程 UDS HTTP 服务）**

`scripts/fake-sidecar.mjs`：
```javascript
// 假 sidecar：UDS HTTP server，供 M2 单测/手动验证（真实 sidecar M4 替换）
import { createServer } from 'node:http';
import { mkdirSync } from 'node:fs';

const path = process.env.DSH_SOCKET ?? '/tmp/dsh-uds-test/dsh.sock';
const dir = path.slice(0, path.lastIndexOf('/'));
mkdirSync(dir, { recursive: true, mode: 0o700 });

const server = createServer((req, res) => {
  if (req.url === '/api/host.describe') {
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ ok: true, method: 'host.describe' }));
    return;
  }
  res.statusCode = 404;
  res.end(JSON.stringify({ ok: false, error: 'not found' }));
});
server.listen(path, () => {
  console.log(`fake sidecar listening on ${path}`);
});
process.on('SIGTERM', () => { server.close(() => process.exit(0)); });
process.on('SIGINT', () => { server.close(() => process.exit(0)); });
```

- [ ] **Step 5: 集成测试（spawn 假 sidecar → dsh_http 命中）**

`src-tauri/src/http_command.rs` 追加（R1 修正：核心逻辑抽成纯函数 `dsh_http_impl(state: AppState, method, path, body)`，命令薄包装——单测直测纯函数，绕开 tauri State 注入难题；AppState 三字段完整构造）：
```rust
#[cfg(test)]
mod integration {
    use super::*;
    use std::process::{Child, Command};
    use std::time::Duration;
    use std::sync::{Arc, Mutex};

    struct Sidecar(Child);

    impl Sidecar {
        fn start() -> Sidecar {
            // R2 修正：用 CARGO_MANIFEST_DIR 锚定绝对路径（cargo test cwd = src-tauri，相对路径找不到根 scripts/）
            let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/fake-sidecar.mjs");
            let child = Command::new("node")
                .arg(script)
                .env("DSH_SOCKET", UDS_PATH)
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
        let _sc = Sidecar::start();
        let client = reqwest::ClientBuilder::new()
            .unix_socket(UDS_PATH)
            .build()
            .unwrap();
        let state = crate::AppState {
            http_client: client,
            uds_path: UDS_PATH.to_string(),
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
```

> R1 修正：`dsh_http_impl` 定义于 `http_command.rs`（pub async fn，参数 `AppState` 按值），`#[tauri::command] dsh_http(state: State<AppState>, ...)` 薄包装调用 `dsh_http_impl(state.inner().clone(), ...)`；`AppState` 需 derive Clone（registry 用 Arc）。集成测试只依赖 `dsh_http_impl`，不依赖 tauri mock 运行时（若 mock 运行时不可达 ACL，测试仍绿）。

- [ ] **Step 6: 运行全部测试**

```bash
cd src-tauri && cargo test 2>&1 | tail -15
```

预期：`test result: ok`，含 uplink_roundtrip（直测 dsh_http_impl）通过。

- [ ] **Step 7: sidecar 重启后重建 client（spec §4.2；R1 修正：补显式任务）**

`src-tauri/src/http_command.rs` 的 `dsh_http_impl` 中，`send().await` 匹配增加 connect 错误分支：`Err(e) if e.is_connect()` → 用 `state.uds_path` 重建 reqwest client（丢弃连接池死连接）并重试一次；再次失败才返回 `Err`。补一条测试（先起假 sidecar 后 kill，验证重试路径触发）。

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/http_command.rs scripts/fake-sidecar.mjs
git commit -m "feat(src-tauri): dsh_http uplink over UDS with input validation"
```

---

### Task 3: downlink 三命令（open/close/cancel stream）

**Files:**
- Create: `src-tauri/src/streams.rs`
- Modify: `src-tauri/src/lib.rs`（注册命令 + AppState）
- Modify: `src-tauri/src/http_command.rs`（AppState 引用）

**Interfaces:**
- Consumes: `UDS_PATH`；`tauri::ipc::Channel<String>`（前端创建后传入）
- Produces:
  - `#[tauri::command] async fn dsh_open_stream(stream: String, channel: Channel<String>, state: State<AppState>) -> Result<u64, String>`——stream ∈ {"mux","host"}；返回 stream_id
  - `#[tauri::command] async fn dsh_close_stream(id: u64, state: State<AppState>) -> Result<(), String>`——幂等 no-op
  - `#[tauri::command] async fn dsh_cancel(id: u64, state: State<AppState>) -> Result<(), String>`——取消在途请求（Task 2 扩展，先留占位）

- [ ] **Step 1: 写失败测试 + 补全 StreamRegistry（R3 修正：Task 2 已建骨架，本步把 `tasks: Mutex<HashMap<u64, ()>>` 升级为 StreamTask 并补 close 方法——注意 Task 2 骨架与 Task 3 补全必须合并为一份代码）**

`src-tauri/src/streams.rs` 补全（替换 Task 2 骨架）：
```rust
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
```

- [ ] **Step 2: 运行确认通过**

```bash
cd src-tauri && cargo test streams 2>&1 | tail -5
```

预期：2 passed。

- [ ] **Step 3: 实现 dsh_open_stream（tokio-tungstenite over UnixStream）**

`src-tauri/src/streams.rs` 追加：
```rust
use tokio::net::UnixStream;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::{client_async_with_config}; // R3 修正：client_async_tls_with_config 在默认 features 下不存在（实证 0.29 cfg 门控）；非 TLS 用 client_async_with_config
use futures_util::StreamExt; // R2 修正：reader.next() 必需

const MUX_PATH: &str = "/api/events.mux";
const HOST_PATH: &str = "/api/events.host";

pub struct StreamTask {
    pub channel: Channel<String>,
    pub handle: tokio::task::JoinHandle<()>,
}

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
        // R3 修正：tasks 是 Mutex<HashMap>，需再 lock 才能 insert
        reg.tasks.lock().map_err(|e| e.to_string())?.insert(id, StreamTask { channel, handle });
    }
    Ok(id)
}

async fn open_ws_over_uds(socket_path: &str, ws_url: &str) -> Result<(tokio_tungstenite::WebSocketStream<UnixStream>, ()), String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let stream = UnixStream::connect(socket_path).await.map_err(|e| e.to_string())?;
    let request = ws_url.into_client_request().map_err(|e| e.to_string())?;
    // R3 修正：非 TLS（UDS 不需要 TLS）——client_async_with_config(request, stream, None) 默认 features 即可用
    let (ws, _resp) = client_async_with_config(request, stream, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok((ws, ()))
}
```

> 注：`connect_async_tls_with_config`/`client_async_tls_with_config` 在 0.29 的 API 名以实际编译为准（0.28/0.29 差异在 `client_async` 与 `client_async_tls` 命名）。**编译报错时按提示改用正确函数名**（`client_async_tls_with_config(request, stream, None, None)` 或 0.29 的 `client_async_with_config`——TLS 关闭时两者皆可）。

- [ ] **Step 4: 注册命令 + AppState**

`src-tauri/src/lib.rs` 全文替换：
```rust
mod http_command;
mod streams;

use http_command::{dsh_http, HttpResponse};
use streams::{dsh_close_stream, dsh_open_stream, StreamRegistry};
use std::sync::{Arc, Mutex};
use tauri::State;

pub struct AppState {
    pub http_client: reqwest::Client,
    pub uds_path: String,
    pub registry: Arc<Mutex<StreamRegistry>>,
}

// R2 修正：dsh_http_impl 按值取 state + state.inner().clone() 需要 Clone
impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(), // reqwest::Client: Clone
            uds_path: self.uds_path.clone(),
            registry: Arc::clone(&self.registry),
        }
    }
}

#[tauri::command]
pub async fn dsh_cancel(id: u64, state: State<'_, AppState>) -> Result<(), String> {
    // Task 4 接在途请求取消；当前幂等 no-op（spec：Rust 不设自身超时，取消由前端信号驱动）
    let _ = id;
    let _ = state;
    Ok(())
}

pub fn run() {
    let uds_path = std::env::var("DSH_SOCKET").unwrap_or_else(|_| http_command::UDS_PATH.to_string());
    let http_client = reqwest::ClientBuilder::new()
        .unix_socket(&uds_path)
        .build()
        .expect("build reqwest client with unix socket");

    tauri::Builder::default()
        .manage(AppState {
            http_client,
            uds_path,
            registry: Arc::new(Mutex::new(StreamRegistry::new())),
        })
        .invoke_handler(tauri::generate_handler![dsh_http, dsh_open_stream, dsh_close_stream, dsh_cancel])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 5: 集成测试（假 sidecar 加 WS 支持）**

`scripts/fake-sidecar.mjs` 追加 WS upgrade 处理（在 createServer 回调前）：
```javascript
import { WebSocketServer } from 'ws';

const wss = new WebSocketServer({ noServer: true });
server.on('upgrade', (req, socket, head) => {
  const pathname = new URL(req.url ?? '/', 'http://dsh').pathname;
  if (pathname === '/api/events.mux' || pathname === '/api/events.host') {
    wss.handleUpgrade(req, socket, head, (ws) => {
      ws.send(JSON.stringify({ type: 'server-request', rpcId: 'fake-1', method: 'host.describe', payload: { ok: true } }));
      ws.on('message', () => {});
    });
  } else {
    socket.destroy();
  }
});
```

> 注：`ws` 是 fake-sidecar 依赖——**R3 修正：安装位置必须在 repo 根**（`scripts/fake-sidecar.mjs` 的 node 解析从 `scripts/` 向上，永远到不了 host-patch/node_modules）：
```bash
npm i -D ws   # repo 根（创建 dsh-desktop/package.json）
```

- [ ] **Step 6: 手动验证（tauri dev 暂不可用，先 cargo 直测核心逻辑；R2 修正：补 WS 端到端测试——M2 阶段 open_stream 不得是死代码）**

`src-tauri/src/streams.rs` 追加 WS 集成测试（spawn WS 版假 sidecar → dsh_open_stream 收帧 → kill 后收到 `""` 哨兵）：
```rust
#[cfg(test)]
mod ws_integration {
    use super::*;
    use std::process::Command;
    use std::time::Duration;

    struct Sidecar(std::process::Child);

    impl Sidecar {
        fn start() -> Sidecar {
            let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/fake-sidecar.mjs");
            let child = Command::new("node").arg(script)
                .env("DSH_SOCKET", crate::http_command::UDS_PATH)
                .spawn().expect("spawn ws sidecar");
            std::thread::sleep(Duration::from_millis(800));
            Sidecar(child)
        }
    }
    impl Drop for Sidecar { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }

    #[tokio::test]
    async fn open_stream_receives_frame_and_end_sentinel() {
        let _sc = Sidecar::start();
        // 直接经 open_ws_over_uds 打开 mux 流，读首帧（fake sidecar 启动即发 host.describe 帧）
        let (mut ws, _) = open_ws_over_uds(crate::http_command::UDS_PATH, "ws://dsh/api/events.mux")
            .await.expect("open ws");
        let first = ws.next().await.expect("first frame").expect("frame ok");
        assert!(matches!(first, WsMessage::Text(_)));
    }
}
```

```bash
# R3 修正：PID 捕获（kill %1 跨 shell 失效）
node scripts/fake-sidecar.mjs &
FAKE_PID=$!
sleep 1
cd src-tauri && cargo test 2>&1 | tail -8
kill $FAKE_PID 2>/dev/null || true
```

预期：测试通过（含 open_stream_receives_frame_and_end_sentinel；`""` 哨兵验证在 kill 场景由 M4 e2e 覆盖）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/streams.rs src-tauri/src/lib.rs scripts/fake-sidecar.mjs
git commit -m "feat(src-tauri): downlink stream open/close with ordered channel delivery"
```

---

### Task 4: 进程管理器（spawn / 退避 / 单实例探测 / 退出序列）

**Files:**
- Create: `src-tauri/src/process.rs`
- Create: `src-tauri/src/state_machine.rs`
- Create: `src-tauri/src/state_machine.rs`（单测：迁移表 11 条路径）
- Modify: `src-tauri/src/lib.rs`（集成进程管理）

**Interfaces:**
- Consumes: `UDS_PATH`；sidecar 命令模板（M4 前用假 sidecar 或 `node fake-sidecar.mjs`）
- Produces:
  - `pub enum AppState2 { Stopped, FirstStarting, Running, Restarting, RestartStopped, Stopping }`（Rust 侧状态机，spec §6）
  - `pub fn decide_restart(transition: &mut RestartCounter, alive_secs: u64) -> RestartDecision`（纯函数：存活<30s 计数+1；≥30s 重置；计数≥5 → Stop）
  - `pub fn spawn_sidecar(node_bin: &str, args: &[&str], cwd: &str, log_file: &std::fs::File) -> Result<tokio::process::Child, std::io::Error>`（内部调用 `process_group(0)` 建独立进程组；R1 修正：签名与 Step 3 实现一致）
  - `pub async fn graceful_shutdown(child: &mut Child, kill_after: Duration)`（SIGTERM 组 → 5s → SIGKILL 组）

- [ ] **Step 1: 写失败测试（退避计数纯函数）**

`src-tauri/src/state_machine.rs`：
```rust
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    Restart,
    Stop,
}

#[derive(Debug, Clone)]
pub struct RestartCounter {
    pub consecutive_failures: u32,
    pub current_delay: Duration,
    pub base: Duration,
}

impl RestartCounter {
    pub fn new() -> Self {
        Self { consecutive_failures: 0, current_delay: Duration::from_secs(1), base: Duration::from_secs(1) }
    }

    /// 规则（spec §6）：存活<30s → 计数+1、延迟 = min(30s, 1s × 2^(n-1))；
    /// 存活≥30s → 重置计数与延迟。计数达 5 → Stop。
    pub fn on_exit(&mut self, alive_secs: u64) -> RestartDecision {
        if alive_secs >= 30 {
            self.consecutive_failures = 0;
            self.current_delay = self.base;
            return RestartDecision::Restart;
        }
        self.consecutive_failures += 1;
        if self.consecutive_failures >= 5 {
            return RestartDecision::Stop;
        }
        let n = self.consecutive_failures as u32; // 1 基
        let delay = self.base.saturating_mul(1u32 << (n.saturating_sub(1))).min(Duration::from_secs(30));
        self.current_delay = delay;
        RestartDecision::Restart
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_failure_delay_1s() {
        let mut c = RestartCounter::new();
        assert_eq!(c.on_exit(5), RestartDecision::Restart);
        assert_eq!(c.current_delay, Duration::from_secs(1));
    }

    #[test]
    fn fourth_failure_delay_8s() {
        let mut c = RestartCounter::new();
        c.on_exit(5); c.on_exit(5); c.on_exit(5);
        assert_eq!(c.on_exit(5), RestartDecision::Restart);
        assert_eq!(c.current_delay, Duration::from_secs(8));
    }

    #[test]
    fn fifth_failure_stops() {
        let mut c = RestartCounter::new();
        for _ in 0..4 { c.on_exit(5); }
        assert_eq!(c.on_exit(5), RestartDecision::Stop);
    }

    #[test]
    fn long_lived_resets_counter() {
        let mut c = RestartCounter::new();
        c.on_exit(5); c.on_exit(5); c.on_exit(5);
        assert_eq!(c.on_exit(30), RestartDecision::Restart); // ≥30s 重置
        assert_eq!(c.consecutive_failures, 0);
        assert_eq!(c.current_delay, Duration::from_secs(1));
    }
}
```

- [ ] **Step 2: 运行确认通过**

```bash
cd src-tauri && cargo test state_machine 2>&1 | tail -5
```

预期：4 passed。

- [ ] **Step 3: 实现进程管理（spawn/组信号/清理）**

`src-tauri/src/process.rs`：
```rust
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::sleep;
use std::process::Stdio;

// R1 修正：spawn 用 std::os::unix::process::CommandExt::process_group(0)（安全 API，无 pre_exec/unsafe），
// 使 sidecar 成为独立进程组组长；graceful_shutdown 的 kill(-pid,...) 才安全。
use std::os::unix::process::CommandExt;

/// spawn sidecar 并使其成为独立进程组组长（spec §4.2：组信号收尾 agent 子进程）。
pub fn spawn_sidecar(
    node_bin: &str,
    args: &[&str],
    cwd: &str,
    log_file: &std::fs::File,
) -> std::io::Result<Child> {
    let mut cmd = Command::new(node_bin);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file.try_clone()?))
        .process_group(0); // 独立进程组（组长 = sidecar 自身 pid）
    cmd.spawn()
}

/// 优雅关闭（spec §4.2 退出序列 ②③④）：SIGTERM(组) → grace → SIGKILL(组)
pub async fn graceful_shutdown(child: &mut Child, grace: Duration) {
    let pid = child.id().unwrap_or(0) as i32;
    if pid > 0 {
        unsafe { libc::kill(-pid, libc::SIGTERM); } // 进程组（setpgid/process_group 后安全）
    }
    let done = tokio::time::timeout(grace, child.wait()).await;
    if done.is_err() {
        if pid > 0 {
            unsafe { libc::kill(-pid, libc::SIGKILL); }
        }
        let _ = child.wait().await;
    }
}

/// 活体探测（spec §4.2 单实例）：connect 成功 → Alive；ENOENT/ECONNREFUSED → Stale。
/// R1 修正：去掉空 marker enum，直接三态枚举。
pub enum ProbeResult {
    Alive,
    Stale,
    Error(String),
}

pub async fn probe_socket(socket_path: &str) -> ProbeResult {
    match tokio::net::UnixStream::connect(socket_path).await {
        Ok(_) => ProbeResult::Alive,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProbeResult::Stale,
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => ProbeResult::Stale,
        Err(e) => ProbeResult::Error(e.to_string()),
    }
}

/// $DSH_HOME 派生 socket 路径（spec §4.2；R4 修正：从 lib.rs 移入——process_manager 引用它）
/// 与 M1 selectSocketPath 主路径一致（$DSH_HOME/run/dsh.sock）；缺省 ~/.dsh
pub fn default_socket_path() -> String {
    let home = std::env::var("DSH_HOME").unwrap_or_else(|_| {
        std::env::var("HOME").map(|h| format!("{h}/.dsh")).unwrap_or_else(|_| "/tmp/dsh-desktop".into())
    });
    format!("{home}/run/dsh.sock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn probe_missing_socket_reports_stale() {
        let r = probe_socket("/tmp/definitely-missing-dsh-test/dsh.sock").await;
        assert!(matches!(r, ProbeResult::Stale));
    }

    #[tokio::test]
    async fn probe_alive_when_listening() {
        let dir = std::env::temp_dir().join(format!("dsh-probe-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("dsh.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let r = probe_socket(sock.to_str().unwrap()).await;
        assert!(matches!(r, ProbeResult::Alive));
        drop(listener);
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn probe_enoent_is_stale() {
        let r = probe_socket("/tmp/nonexistent-dsh-probe/dsh.sock").await;
        assert!(matches!(r, ProbeResult::Stale));
    }

    #[tokio::test]
    async fn probe_ec_onnrefused_is_stale() {
        let dir = std::env::temp_dir().join(format!("dsh-probe-refused-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("dsh.sock");
        {
            let listener = tokio::net::UnixListener::bind(&sock).unwrap();
            drop(listener); // 关闭监听，文件残留 → ECONNREFUSED
        }
        let r = probe_socket(sock.to_str().unwrap()).await;
        assert!(matches!(r, ProbeResult::Stale));
        let _ = fs::remove_dir_all(&dir);
    }
}
```

> 注：`process_group(0)` 来自 `std::os::unix::process::CommandExt`（Rust 1.64+），tokio::process::Command 转发实现；无需 pre_exec/unsafe。

- [ ] **Step 4: 状态机迁移表（spec §6 11 条路径）**

`src-tauri/src/state_machine.rs` 追加：
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState2 {
    Stopped,
    FirstStarting,
    Running,
    Restarting,
    RestartStopped,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    Start,            // App 启动 → spawn
    SocketReady,      // socket 可达 → running
    UnexpectedExit,   // 任意非 App 驱动退出（含 exit 0）
    BackoffElapsed,   // 退避到期 → respawn
    FirstStartFailed, // 首启 pre-ready 失败（从未达过 socket-ready）
    RetryFromDialog,  // 对话框重试
    RetryFromTray,    // restart-stopped 托盘重试（重置计数）
    UserQuit,         // Cmd+Q / 托盘退出
}

// R4 修正：ever_ready 由调用方经事件选择（FirstStartFailed vs UnexpectedExit）表达；
// alive_secs 传入 on_exit（≥30s 重置规则可经状态机路径表达，不再硬编码 0）
pub fn transition(state: AppState2, event: AppEvent, counter: &mut RestartCounter, alive_secs: u64) -> AppState2 {
    match (state, event) {
        (AppState2::Stopped, AppEvent::Start) => AppState2::FirstStarting,
        (AppState2::FirstStarting, AppEvent::SocketReady) => AppState2::Running,
        (AppState2::FirstStarting, AppEvent::FirstStartFailed) => AppState2::Stopped, // 首启对话框（UI 层，不计数）
        (AppState2::Running, AppEvent::UnexpectedExit) => match counter.on_exit(alive_secs) {
            RestartDecision::Restart => AppState2::Restarting,
            RestartDecision::Stop => AppState2::RestartStopped,
        },
        (AppState2::Restarting, AppEvent::BackoffElapsed) => AppState2::FirstStarting,
        (AppState2::RestartStopped, AppEvent::RetryFromTray) => {
            counter.consecutive_failures = 0;
            counter.current_delay = counter.base;
            AppState2::FirstStarting
        }
        (_, AppEvent::UserQuit) => AppState2::Stopping,
        _ => state, // 未定义迁移保持原状态（幂等）
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn start_to_running() {
        let mut c = RestartCounter::new();
        assert_eq!(transition(AppState2::Stopped, AppEvent::Start, &mut c, 0), AppState2::FirstStarting);
        assert_eq!(transition(AppState2::FirstStarting, AppEvent::SocketReady, &mut c, 0), AppState2::Running);
    }

    #[test]
    fn unexpected_exit_backoff_then_stop() {
        let mut c = RestartCounter::new();
        let s = transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c, 5);
        assert_eq!(s, AppState2::Restarting);
        assert_eq!(transition(AppState2::Restarting, AppEvent::BackoffElapsed, &mut c, 0), AppState2::FirstStarting);
    }

    #[test]
    fn long_lived_crash_resets_via_transition() {
        let mut c = RestartCounter::new();
        c.on_exit(5); c.on_exit(5); // 2 次短命失败
        let s = transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c, 60); // 存活≥30s
        assert_eq!(s, AppState2::Restarting);
        assert_eq!(c.consecutive_failures, 0); // R4 修正：≥30s 重置经状态机路径验证
    }

    #[test]
    fn five_failures_stops() {
        let mut c = RestartCounter::new();
        for _ in 0..5 {
            let s = transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c, 0);
            if s == AppState2::Restarting {
                transition(AppState2::Restarting, AppEvent::BackoffElapsed, &mut c, 0);
            }
        }
        assert_eq!(transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c, 0), AppState2::RestartStopped);
    }

    #[test]
    fn quit_anywhere_stops() {
        let mut c = RestartCounter::new();
        for s in [AppState2::Running, AppState2::Restarting, AppState2::FirstStarting] {
            assert_eq!(transition(s, AppEvent::UserQuit, &mut c, 0), AppState2::Stopping);
        }
    }
}
```

- [ ] **Step 5: 运行全部测试（R2 修正：AppState 定义提前到 Task 2——集成测试引用它）**

> R2 修正：`AppState` 三字段结构 + `impl Clone` 的定义**前移至 Task 2 Step 3**（http_command.rs 需要的 State 类型），Task 3 的 lib.rs 不再定义而是复用。顺序：Task 2 先建 AppState → dsh_http_impl → 集成测试 → Task 3 streams 注册表。

```bash
cd src-tauri && cargo test 2>&1 | tail -10
```

预期：state_machine（4+4=8 passed）+ streams + http 全绿。

- [ ] **Step 6: 手动验证退避节奏（kill -9 → 1s→2s→4s→8s→停止）**

用带日志的假 sidecar + 测试驱动脚本（M2 验收 ②）：
```bash
# 启动 tauri（或最小 runner）并观察进程管理日志；M2 阶段先验证纯函数（已完成），
# 完整 spawn 循环在 M4 集成（进程管理器接真实 sidecar）时验证。
# 此处验证 graceful_shutdown 进程组信号：
cd src-tauri && cargo test process 2>&1 | tail -5
```

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/process.rs src-tauri/src/state_machine.rs
git commit -m "feat(src-tauri): process manager with backoff state machine"
```

---

### Task 5: 大 body 阈值重测（M2 验收 ④）+ 退出序列接线

**Files:**
- Create: `scripts/bench-big-body.mjs`
- Modify: `src-tauri/src/lib.rs`（RunEvent::ExitRequested 接线）

**Interfaces:**
- Consumes: Task 2/4 产物
- Produces: 150 MiB 数据点记录（spec §6 错误处理表）；退出序列实现

- [ ] **Step 1: 150 MiB 数据点（R2 修正：经 Rust dsh_http_impl 转发测量，不直打假 sidecar——spec 数据点意图是 invoke 大 payload 代价）**

`scripts/bench-big-body.mjs`：
```javascript
// 大 body 阈值重测（spec M2 验收 ④）：150 MiB 响应经 Rust 管道转发耗时
import { createServer } from 'node:http';
const path = process.env.DSH_SOCKET ?? '/tmp/dsh-uds-test/dsh.sock';
const SIZE = 150 * 1024 * 1024;
const chunk = Buffer.alloc(64 * 1024, 0x61);
const server = createServer((req, res) => {
  let sent = 0;
  res.writeHead(200, { 'Content-Type': 'application/octet-stream' });
  const timer = setInterval(() => {
    res.write(chunk);
    sent += chunk.length;
    if (sent >= SIZE) { clearInterval(timer); res.end(); }
  }, 5);
});
server.listen(path, () => console.log(`big-body server on ${path}`));
process.on('SIGTERM', () => server.close(() => process.exit(0)));
```

```bash
# R2 修正：通过 Rust dsh_http_impl 的测试/CLI 通道转发，测 invoke 路径真实代价
node scripts/bench-big-body.mjs &
sleep 1
# 方式 A：cargo test 专用 bench（bench_big_body 集成测试，spawn sidecar 后经 dsh_http_impl 拉 150MiB 计时）
cd src-tauri && cargo test bench_big_body 2>&1 | tail -5
kill %1
```

> 若实现 bench 测试成本高，退化为方式 B：直连 UDS 的 curl 计时仅作参考值，并在 `docs/bench-notes.md` 标注「非 invoke 路径，需 M4 复测」——**不得冒充 M2 验收④数据点**。
记录耗时到 `docs/bench-notes.md`（M2 数据点）。

- [ ] **Step 2: 退出序列接线（lib.rs 扩展；R2 修正：替换 `.run(...)` 而非在其后追加——追加会形成非法 Rust）**

`src-tauri/src/lib.rs` 的 `.run(tauri::generate_context!())` **替换**为：
```rust
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
```

> 注：`.build()` 前必须 `use tauri::Manager;`（M4 Task 1 已有）。此占位在 M4 Task 5 Step 4 替换为完整 `shutdown_sequence`（含 SIGTERM→5s→SIGKILL→unlink）。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/lib.rs scripts/bench-big-body.mjs
git commit -m "feat(src-tauri): exit sequence hook + big body benchmark"
```

---

## M2 完成检查（对照 spec §10 M2 验收）

- [ ] ① Rust 管道/进程管理单测全绿（close_stream 幂等、输入校验、退避计数与重置、退出序列取消定时器、进程组信号、活体探测三分支、describe 超时）
- [ ] ② 手动：假 sidecar 下 invoke 转发往返成功；kill -9 → 退避节奏 1s→2s→4s→8s→停止（M4 集成验证）
- [ ] ③ Cmd+Q 退出序列 SIGTERM→5s→SIGKILL 可观察（M4 集成验证）
- [ ] ④ 大 body 阈值重测（150 MiB 数据点已记录）
