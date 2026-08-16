# M2 数据点 — 150 MiB 大 body 经 Rust 管道

日期：2026-08-16
测量：`dsh_http_impl`（reqwest unix_socket，GET /api/big，150 MiB 响应）
结果：**14.9 s**（含 fake sidecar 以 64 KiB 块 5ms 间隔写入——写入节奏是主要瓶颈，非管道）

要点：
- 响应完整到达（body.len() == 150×1024×1024 断言通过）。
- spec §6 错误处理表：carrier 侧 body 上限 160 MiB（DEFAULT_MAX_REQUEST_BODY_BYTES）；Rust 侧同样设 160 MiB 上限（DeepSec L3，防 invoke 超大 body OOM）。
- 实际 UX 中 150 MiB 属 session.export 极端场景；正常 unary 载荷远小于此。
