// e2e smoke（spec §7 CI 自动化层）：零 WebDriver；socket 可达性断言。
// 由 e2e-smoke.sh 驱动（先起 sidecar，再跑本测试）；bare cargo test 无 DSH_SOCKET 时跳过。
use std::process::Command;

#[test]
fn sidecar_socket_reachable() {
    let Ok(sock) = std::env::var("DSH_SOCKET") else {
        eprintln!("skipping: DSH_SOCKET not set (run via e2e-smoke.sh)");
        return;
    };
    let out = Command::new("curl")
        .args([
            "--unix-socket", &sock,
            "-s", "-o", "/dev/null", "-w", "%{http_code}",
            "-X", "POST",
            "-H", "Content-Type: application/json",
            "-d", r#"{"type":"server-request","rpcId":"smoke-1","method":"host.describe","payload":{}}"#,
            "http://dsh/api/host.describe",
        ])
        .output()
        .expect("curl must run");
    let code = String::from_utf8_lossy(&out.stdout);
    // 无 key 时 describe 可能返回业务错误（非 2xx），但连接必须建立——http_code 非 000 即可达
    assert_ne!(code, "000", "socket unreachable: {}", String::from_utf8_lossy(&out.stderr));
}
