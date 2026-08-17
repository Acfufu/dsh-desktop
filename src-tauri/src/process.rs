use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::sleep;
use std::process::Stdio;

// R1 修正：spawn 用 std::os::unix::process::CommandExt::process_group(0)（安全 API，无 pre_exec/unsafe），
// 使 sidecar 成为独立进程组组长；graceful_shutdown 的 kill(-pid,...) 才安全。
use std::os::unix::process::CommandExt;

/// spawn sidecar 并使其成为独立进程组组长（spec §4.2：组信号收尾 agent 子进程）。
/// R6 修正：显式传 `$DSH_HOME`——GUI 启动（Finder/LaunchServices）无 shell env，继承环境里没有
/// DSH_HOME；carrier 的 `selectSocketPath` 读 `process.env.DSH_HOME`，未设时回退
/// `os.tmpdir()/dsh-<uid>/dsh.sock`，而 Rust `default_socket_path()` 回退 `~/.dsh/run/dsh.sock`——
/// 两侧路径不一致 → 全部 dsh_http 连接 ENOENT → 「transport after rebuild」。显式传入使两侧
/// 从同一真源派生 socket 路径。
pub fn spawn_sidecar(
    node_bin: &str,
    args: &[&str],
    cwd: &str,
    dsh_home: &str,
    log_file: &std::fs::File,
) -> std::io::Result<Child> {
    let mut cmd = Command::new(node_bin);
    cmd.args(args)
        .current_dir(cwd)
        .env("LC_ALL", "en_US.UTF-8") // spec §4.2：显式 locale 防乱码
        .env("DSH_HOME", dsh_home) // R6：socket 路径唯一真源（见上）
        .stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file.try_clone()?))
        .process_group(0); // 独立进程组（组长 = sidecar 自身 pid）
    cmd.spawn()
}

/// 优雅关闭（spec §4.2 退出序列 ②③④）：SIGTERM(组) → grace → SIGKILL(组)
/// DeepSec L3 修正：先探测 pid 是否仍属于我们（kill(pid,0)），避免 PID 复用后误杀无关进程组；
/// SIGKILL 后 wait 加超时，防不可中断进程挂死退出序列。
pub async fn graceful_shutdown(child: &mut Child, grace: Duration) {
    let pid = child.id().unwrap_or(0) as i32;
    if pid > 0 && unsafe { libc::kill(pid, 0) } == 0 {
        unsafe { libc::kill(-pid, libc::SIGTERM); } // 进程组（process_group 后安全）
    }
    let done = tokio::time::timeout(grace, child.wait()).await;
    if done.is_err() {
        if pid > 0 && unsafe { libc::kill(pid, 0) } == 0 {
            unsafe { libc::kill(-pid, libc::SIGKILL); }
        }
        // DeepSec L3：SIGKILL 后 wait 也加超时（10s），不可中断进程不挂死退出
        let _ = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;
    }
}

/// 活体探测（spec §4.2 单实例）：connect 成功 → Alive；ENOENT/ECONNREFUSED → Stale。
#[derive(Debug)]
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
        tokio::time::sleep(Duration::from_millis(100)).await;
        let r = probe_socket(sock.to_str().unwrap()).await;
        assert!(matches!(r, ProbeResult::Stale));
        let _ = fs::remove_dir_all(&dir);
    }

    // R6 回归：GUI 启动无 shell env 时 DSH_HOME 必须显式传入——否则 carrier 的 selectSocketPath
    // 回退 os.tmpdir()/dsh-<uid>/dsh.sock，与 Rust default_socket_path()（$DSH_HOME/run/dsh.sock）
    // 不一致 → 全部 dsh_http「transport after rebuild」。
    #[tokio::test]
    async fn spawn_passes_dsh_home_to_child_env() {
        let dir = std::env::temp_dir().join(format!("dsh-spawn-env-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join("sidecar.log");
        let log = fs::File::create(&log_path).unwrap();
        let home = "/tmp/dsh-spawn-env-home";
        let mut child = spawn_sidecar(
            "node",
            &["-e", "console.log(process.env.DSH_HOME)"],
            dir.to_str().unwrap(),
            home,
            &log,
        )
        .unwrap();
        let _ = child.wait().await;
        let out = fs::read_to_string(&log_path).unwrap();
        assert!(out.contains(home), "child env must carry DSH_HOME, got: {out}");
        let _ = fs::remove_dir_all(&dir);
    }
}
